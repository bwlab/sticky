//! The text of a note: the buffer, its formatting tags, lists, links and images.
//!
//! One `NoteBuffer` serves both the editable window and the read-only previews
//! of the "All Notes" cards; `editable` tells them apart.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gdk, glib, pango};
use gtk::glib::translate::IntoGlib;
use gtk::glib::clone;
use sticky_core::{links, NoteImage, Tag, IMAGE_PLACEHOLDER};

use crate::images::{
    clipboard_has_image, load_texture, read_clipboard_textures, save_texture, textures_from_files,
};
use crate::note_object::NoteObject;

/// Checklist and bullet items are stored as plain characters at the start of a
/// line. This keeps notes readable when copied elsewhere, and means the note
/// format doesn't change: a checklist survives saving, searching and the
/// read-only previews shown in the "All Notes" window for free.
pub const UNCHECKED: &str = "☐";
pub const CHECKED: &str = "☑";

const CHECK_PREFIXES: [&str; 2] = ["☐ ", "☑ "];
const BULLET_PREFIXES: [&str; 1] = ["• "];
const LIST_PREFIXES: [&str; 3] = ["☐ ", "☑ ", "• "];
const ZWSP: char = '\u{200B}';

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextSize {
    Normal,
    Small,
    Large,
    Larger,
}

impl TextSize {
    pub fn name(self) -> &'static str {
        match self {
            TextSize::Normal => "normal",
            TextSize::Small => "small",
            TextSize::Large => "large",
            TextSize::Larger => "larger",
        }
    }

    pub fn from_name(name: &str) -> TextSize {
        match name {
            "small" => TextSize::Small,
            "large" => TextSize::Large,
            "larger" => TextSize::Larger,
            _ => TextSize::Normal,
        }
    }
}

/// Every tag a note's text can carry.
#[derive(Clone)]
pub struct Tags {
    pub bold: gtk::TextTag,
    pub underline: gtk::TextTag,
    pub italic: gtk::TextTag,
    pub strikethrough: gtk::TextTag,
    pub monospace: gtk::TextTag,
    pub highlight: gtk::TextTag,
    pub header: gtk::TextTag,
    pub small: gtk::TextTag,
    pub large: gtk::TextTag,
    pub larger: gtk::TextTag,
    pub link: gtk::TextTag,
    /// Draws the list markers larger than the text they sit next to, so a
    /// checkbox looks like something you can tick. Nameless on purpose: the
    /// note format stores tags by name, and this one is presentation the app
    /// re-applies on its own.
    pub marker: gtk::TextTag,
}

impl Tags {
    fn new(table: &gtk::TextTagTable) -> Tags {
        let tag = |name: Option<&str>| gtk::TextTag::new(name);
        let t = Tags {
            bold: tag(Some("bold")),
            underline: tag(Some("underline")),
            italic: tag(Some("italic")),
            strikethrough: tag(Some("strikethrough")),
            monospace: tag(Some("monospace")),
            highlight: tag(Some("highlight")),
            header: tag(Some("header")),
            small: tag(Some("small")),
            large: tag(Some("large")),
            larger: tag(Some("larger")),
            link: tag(Some("link")),
            marker: tag(None),
        };

        t.bold.set_weight(pango::Weight::Bold.into_glib());
        t.underline.set_underline(pango::Underline::Single);
        t.italic.set_style(pango::Style::Italic);
        t.strikethrough.set_strikethrough(true);
        t.monospace.set_family(Some("Monospace"));

        // the highlighter also sets a foreground, as the notes it is used on can
        // be dark, and white on yellow is unreadable
        t.highlight.set_background(Some("#f9f06b"));
        t.highlight.set_foreground(Some("#241f31"));

        t.header.set_scale(1.5);
        t.header.set_pixels_above_lines(14);
        t.header.set_pixels_below_lines(4);

        // the same steps Pango uses for small/large/x-large
        t.small.set_scale(1.0 / 1.2);
        t.large.set_scale(1.2);
        t.larger.set_scale(1.2 * 1.2);

        t.link.set_underline(pango::Underline::Single);
        t.marker.set_scale(1.4);

        for (_, tag) in t.actions() {
            table.add(&tag);
        }
        for (_, tag) in t.sizes() {
            table.add(&tag);
        }
        table.add(&t.link);
        table.add(&t.marker);
        t
    }

    /// The tags the formatting actions toggle.
    pub fn actions(&self) -> Vec<(&'static str, gtk::TextTag)> {
        vec![
            ("bold", self.bold.clone()),
            ("underline", self.underline.clone()),
            ("italic", self.italic.clone()),
            ("strikethrough", self.strikethrough.clone()),
            ("monospace", self.monospace.clone()),
            ("highlight", self.highlight.clone()),
            ("header", self.header.clone()),
        ]
    }

    pub fn sizes(&self) -> Vec<(TextSize, gtk::TextTag)> {
        vec![
            (TextSize::Small, self.small.clone()),
            (TextSize::Large, self.large.clone()),
            (TextSize::Larger, self.larger.clone()),
        ]
    }

    /// Tags that all change the size of the text, so only one of them may be
    /// applied to a range at a time.
    fn scale_tags(&self) -> Vec<gtk::TextTag> {
        let mut v = vec![self.header.clone()];
        v.extend(self.sizes().into_iter().map(|(_, t)| t));
        v
    }
}

type Handler<T> = RefCell<Option<Rc<dyn Fn(T)>>>;

struct Inner {
    buffer: gtk::TextBuffer,
    tags: Tags,
    editable: bool,
    note: RefCell<Option<NoteObject>>,
    note_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    /// The view the user works in, needed for the theme colours of links.
    view: RefCell<Option<glib::WeakRef<gtk::TextView>>>,

    // writable only
    loading: Cell<bool>,
    restoring: Cell<bool>,
    anchors: RefCell<HashMap<gtk::TextChildAnchor, String>>,
    image_views: RefCell<Vec<(glib::WeakRef<gtk::TextView>, HashMap<gtk::TextChildAnchor, gtk::Widget>)>>,

    last_link: Cell<Option<(i32, i32)>>,

    on_tag_toggle: Handler<(&'static str, bool)>,
    on_selection_changed: Handler<()>,
    on_link_selected: Handler<Option<String>>,
}

#[derive(Clone)]
pub struct NoteBuffer {
    inner: Rc<Inner>,
}

/// The same tag toggle announced under a name that isn't `&'static`.
fn static_name(name: &str) -> &'static str {
    match name {
        "bold" => "bold",
        "underline" => "underline",
        "italic" => "italic",
        "strikethrough" => "strikethrough",
        "monospace" => "monospace",
        "highlight" => "highlight",
        "header" => "header",
        "small" => "small",
        "large" => "large",
        "larger" => "larger",
        _ => "link",
    }
}

impl NoteBuffer {
    pub fn new(editable: bool) -> NoteBuffer {
        let buffer = gtk::TextBuffer::new(None);
        let tags = Tags::new(&buffer.tag_table());
        let nb = NoteBuffer {
            inner: Rc::new(Inner {
                buffer,
                tags,
                editable,
                note: RefCell::new(None),
                note_handlers: RefCell::new(vec![]),
                view: RefCell::new(None),
                loading: Cell::new(false),
                restoring: Cell::new(false),
                anchors: RefCell::new(HashMap::new()),
                image_views: RefCell::new(vec![]),
                last_link: Cell::new(None),
                on_tag_toggle: RefCell::new(None),
                on_selection_changed: RefCell::new(None),
                on_link_selected: RefCell::new(None),
            }),
        };
        nb.connect_buffer_signals();
        nb
    }

    pub fn buffer(&self) -> &gtk::TextBuffer {
        &self.inner.buffer
    }

    pub fn tags(&self) -> &Tags {
        &self.inner.tags
    }

    pub fn note(&self) -> Option<NoteObject> {
        self.inner.note.borrow().clone()
    }

    pub fn on_tag_toggle(&self, f: impl Fn(&'static str, bool) + 'static) {
        *self.inner.on_tag_toggle.borrow_mut() = Some(Rc::new(move |(n, a)| f(n, a)));
    }

    pub fn on_selection_changed(&self, f: impl Fn() + 'static) {
        *self.inner.on_selection_changed.borrow_mut() = Some(Rc::new(move |()| f()));
    }

    /// Called with the text of the link under the cursor, or `None` when there is none.
    pub fn on_link_selected(&self, f: impl Fn(Option<String>) + 'static) {
        *self.inner.on_link_selected.borrow_mut() = Some(Rc::new(f));
    }

    fn emit_tag_toggle(&self, name: &str, active: bool) {
        let h = self.inner.on_tag_toggle.borrow().clone();
        if let Some(h) = h {
            h((static_name(name), active));
        }
    }

    pub fn attach_view(&self, view: &gtk::TextView) {
        *self.inner.view.borrow_mut() = Some(view.downgrade());
    }

    // ---- the note --------------------------------------------------------

    pub fn set_note(&self, note: &NoteObject) {
        self.remove_listeners();
        *self.inner.note.borrow_mut() = Some(note.clone());

        if self.inner.editable {
            self.clear_tags();
            self.inner.loading.set(true);
            self.inner.buffer.set_text(&note.content());
            self.inner.loading.set(false);
            self.restore_images(note);
            self.init_tags(&note.tags());
            self.update_markers();
            self.update_link_tag_color();
        } else {
            self.show_content();
            self.update_link_tag_color();
        }
        self.init_listeners();
    }

    fn init_listeners(&self) {
        let Some(note) = self.note() else { return };
        let weak = Rc::downgrade(&self.inner);
        let upgrade = move |weak: &Weak<Inner>| weak.upgrade().map(|inner| NoteBuffer { inner });

        let mut ids = vec![];
        ids.push(note.connect_style_notify(clone!(
            #[strong(rename_to = w)]
            weak,
            move |_| {
                if let Some(nb) = upgrade(&w) {
                    nb.update_link_tag_color();
                }
            }
        )));

        if !self.inner.editable {
            ids.push(note.connect_content_notify(clone!(
                #[strong(rename_to = w)]
                weak,
                move |_| {
                    if let Some(nb) = upgrade(&w) {
                        nb.show_content();
                    }
                }
            )));
            ids.push(note.connect_tag_list_notify(clone!(
                #[strong(rename_to = w)]
                weak,
                move |_| {
                    if let Some(nb) = upgrade(&w) {
                        nb.reapply_tags();
                    }
                }
            )));
        }
        *self.inner.note_handlers.borrow_mut() = ids;
    }

    pub fn remove_listeners(&self) {
        let ids: Vec<_> = self.inner.note_handlers.borrow_mut().drain(..).collect();
        if let Some(note) = self.note() {
            for id in ids {
                note.disconnect(id);
            }
        }
    }

    fn reapply_tags(&self) {
        self.clear_tags();
        if let Some(note) = self.note() {
            self.init_tags(&note.tags());
        }
        self.update_markers();
    }

    /// The content of the note: the text, with a placeholder for each image, so
    /// that the offsets of tags and images match the ones in the buffer.
    pub fn content_text(&self) -> String {
        let b = &self.inner.buffer;
        b.slice(&b.start_iter(), &b.end_iter(), true).to_string()
    }

    // ---- connecting the buffer ----------------------------------------------

    fn connect_buffer_signals(&self) {
        let b = &self.inner.buffer;
        let weak = Rc::downgrade(&self.inner);

        b.connect_mark_set(clone!(
            #[strong(rename_to = w)]
            weak,
            move |buffer, _loc, mark| {
                let Some(inner) = w.upgrade() else { return };
                let nb = NoteBuffer { inner };
                if nb.note().is_none() {
                    return;
                }

                let name = mark.name();
                if name.as_deref() == Some("insert")
                    || name.as_deref() == Some("selection_bound")
                    || buffer.iter_at_mark(mark) == buffer.iter_at_mark(&buffer.get_insert())
                {
                    let h = nb.inner.on_selection_changed.borrow().clone();
                    if let Some(h) = h {
                        h(());
                    }
                }
                nb.check_link_selected();

                if nb.inner.editable {
                    nb.sync_tags_to_note();
                }
            }
        ));

        if self.inner.editable {
            b.connect_closure(
                "insert-text",
                true,
                glib::closure_local!(
                    #[strong(rename_to = w)]
                    weak,
                    move |buffer: gtk::TextBuffer, loc: gtk::TextIter, text: String, _len: i32| {
                        if let Some(inner) = w.upgrade() {
                            NoteBuffer { inner }.on_text_inserted(&buffer, &loc, &text);
                        }
                    }
                ),
            );

            b.connect_changed(clone!(
                #[strong(rename_to = w)]
                weak,
                move |_| {
                    let Some(inner) = w.upgrade() else { return };
                    // images are being put back one by one, or the note is being
                    // loaded: the content hasn't changed
                    if inner.restoring.get() || inner.loading.get() {
                        return;
                    }
                    NoteBuffer { inner }.on_content_changed();
                }
            ));
        }
    }

    fn sync_tags_to_note(&self) {
        let Some(note) = self.note() else { return };
        let tags = self.get_tags();
        if compare_tags(&tags, &note.tags()) {
            return;
        }
        note.set_tags(tags);
    }

    // ---- links -----------------------------------------------------------

    fn check_link_selected(&self) {
        let link = &self.inner.tags.link;
        let b = &self.inner.buffer;

        if let Some(offset) = self.has_tag(link) {
            let mut start = b.iter_at_offset(offset);
            if !start.starts_tag(Some(link)) {
                start.backward_to_tag_toggle(Some(link));
            }
            let mut end = start;
            end.forward_to_tag_toggle(Some(link));

            let span = (start.offset(), end.offset());
            if self.inner.last_link.get() != Some(span) {
                let text: String = b.text(&start, &end, false).chars().filter(|c| *c != ZWSP).collect();
                self.inner.last_link.set(Some(span));
                let h = self.inner.on_link_selected.borrow().clone();
                if let Some(h) = h {
                    h(Some(text));
                }
            }
        } else if self.inner.last_link.get().is_some() {
            self.inner.last_link.set(None);
            let h = self.inner.on_link_selected.borrow().clone();
            if let Some(h) = h {
                h(None);
            }
        }
    }

    pub fn update_link_tag_color(&self) {
        let Some(note) = self.note() else { return };
        let fallback = "#0000ff".to_string();
        let view = self.inner.view.borrow().as_ref().and_then(|v| v.upgrade());
        let Some(view) = view else {
            self.inner.tags.link.set_foreground(Some(&fallback));
            return;
        };

        let name = if adw::StyleManager::default().is_dark() {
            "accent_fg_color".to_string()
        } else {
            format!("link_color_{}", note.style_enum().name())
        };
        #[allow(deprecated)]
        let color = view
            .style_context()
            .lookup_color(&name)
            .map(|c| c.to_string())
            .unwrap_or(fallback);
        self.inner.tags.link.set_foreground(Some(&color));
    }

    fn clear_links(&self) {
        let b = &self.inner.buffer;
        b.remove_tag(&self.inner.tags.link, &b.start_iter(), &b.end_iter());
        self.emit_tag_toggle("link", false);
    }

    fn update_links(&self) {
        let text = self.content_text();
        self.clear_links();
        let b = &self.inner.buffer;
        for link in links::find(&text) {
            let start = b.iter_at_offset(link.start as i32);
            let end = b.iter_at_offset(link.end as i32);
            b.apply_tag(&self.inner.tags.link, &start, &end);
        }
        self.emit_tag_toggle("link", true);
    }

    // ---- tags ------------------------------------------------------------

    pub fn init_tags(&self, tags: &[Tag]) {
        let b = &self.inner.buffer;
        for tag in tags {
            let Some(t) = b.tag_table().lookup(&tag.name) else { continue };
            b.apply_tag(&t, &b.iter_at_offset(tag.start as i32), &b.iter_at_offset(tag.end as i32));
        }
    }

    /// The offset of the first character of the selection (or the one at the
    /// cursor) that has `tag`.
    pub fn has_tag(&self, tag: &gtk::TextTag) -> Option<i32> {
        let b = &self.inner.buffer;
        let (mut start, end) = match b.selection_bounds() {
            Some(bounds) => bounds,
            None => {
                let start = b.iter_at_mark(&b.get_insert());
                let mut end = start;
                end.forward_cursor_position();
                (start, end)
            }
        };

        loop {
            if start.has_tag(tag) {
                return Some(start.offset());
            }
            start.forward_char();
            if start >= end {
                return None;
            }
        }
    }

    /// The range the next formatting operation applies to: the selection, or a
    /// spot at the cursor the user can type into.
    fn format_range(&self) -> (gtk::TextIter, gtk::TextIter) {
        let b = &self.inner.buffer;
        if let Some(bounds) = b.selection_bounds() {
            return bounds;
        }
        // If the user has not selected anything, zero-width spaces are inserted
        // around the cursor and marked as the start and end of the selection.
        b.insert_at_cursor("\u{200B}\u{200B}\u{200B}");
        let cursor = b.cursor_position();
        let start = b.iter_at_offset(cursor - 3);
        let end = b.iter_at_offset(cursor - 1);
        let mut place = b.iter_at_offset(cursor);
        place.backward_chars(2);
        b.place_cursor(&place);
        (start, end)
    }

    fn conflicting_tags(&self, tag: &gtk::TextTag) -> Vec<gtk::TextTag> {
        let scale = self.inner.tags.scale_tags();
        if !scale.contains(tag) {
            return vec![];
        }
        scale.into_iter().filter(|o| o != tag).collect()
    }

    pub fn apply_tag(&self, tag: &gtk::TextTag) {
        let b = &self.inner.buffer;
        let (start, end) = self.format_range();
        let name = tag.name().map(|n| n.to_string()).unwrap_or_default();

        let had = self.has_tag(tag).is_some();
        if had {
            b.remove_tag(tag, &start, &end);
        } else {
            for other in self.conflicting_tags(tag) {
                b.remove_tag(&other, &start, &end);
                self.emit_tag_toggle(other.name().as_deref().unwrap_or(""), false);
            }
            b.apply_tag(tag, &start, &end);
        }
        self.emit_tag_toggle(&name, !had);

        if self.inner.editable {
            if let Some(note) = self.note() {
                let tags = self.get_tags();
                if !compare_tags(&tags, &note.tags()) {
                    note.set_tags(tags);
                    note.touch();
                }
            }
        }
    }

    pub fn text_size(&self) -> TextSize {
        for (size, tag) in self.inner.tags.sizes() {
            if self.has_tag(&tag).is_some() {
                return size;
            }
        }
        TextSize::Normal
    }

    pub fn set_text_size(&self, size: TextSize) {
        let b = &self.inner.buffer;
        let (start, end) = self.format_range();

        for (s, tag) in self.inner.tags.sizes() {
            b.remove_tag(&tag, &start, &end);
            self.emit_tag_toggle(s.name(), false);
        }
        // a header has a size of its own, so picking a size drops it
        b.remove_tag(&self.inner.tags.header, &start, &end);
        self.emit_tag_toggle("header", false);

        if let Some((_, tag)) = self.inner.tags.sizes().into_iter().find(|(s, _)| *s == size) {
            b.apply_tag(&tag, &start, &end);
            self.emit_tag_toggle(size.name(), true);
        }
    }

    pub fn clear_tags(&self) {
        let b = &self.inner.buffer;
        let (start, end) = (b.start_iter(), b.end_iter());
        for (name, tag) in self.inner.tags.actions() {
            b.remove_tag(&tag, &start, &end);
            self.emit_tag_toggle(name, false);
        }
        for (size, tag) in self.inner.tags.sizes() {
            b.remove_tag(&tag, &start, &end);
            self.emit_tag_toggle(size.name(), false);
        }
    }

    /// Every named tag with the range it covers; nameless tags are styling the
    /// app applies itself, not part of the note.
    pub fn get_tags(&self) -> Vec<Tag> {
        let b = &self.inner.buffer;
        let mut named = vec![];
        b.tag_table().foreach(|tag| {
            if let Some(name) = tag.name() {
                named.push((name.to_string(), tag.clone()));
            }
        });

        let mut out = vec![];
        for (name, tag) in named {
            let mut iter = b.start_iter();
            loop {
                if iter.starts_tag(Some(&tag)) {
                    let begin = iter.offset();
                    iter.forward_to_tag_toggle(Some(&tag));
                    out.push(Tag { name: name.clone(), start: begin as u32, end: iter.offset() as u32 });
                }
                if !iter.forward_to_tag_toggle(Some(&tag)) {
                    break;
                }
            }
        }
        out
    }

    // ---- lists -----------------------------------------------------------

    fn line_text(&self, line: i32) -> Option<String> {
        let b = &self.inner.buffer;
        let start = b.iter_at_line(line)?;
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        Some(b.text(&start, &end, false).to_string())
    }

    /// The list marker `line` starts with, if it is one of `prefixes`.
    fn line_prefix(&self, line: i32, prefixes: &[&'static str]) -> Option<&'static str> {
        let text = self.line_text(line)?;
        prefixes.iter().copied().find(|p| text.starts_with(p))
    }

    fn selected_lines(&self) -> (i32, i32) {
        let b = &self.inner.buffer;
        match b.selection_bounds() {
            None => {
                let line = b.iter_at_mark(&b.get_insert()).line();
                (line, line)
            }
            Some((start, end)) => (start.line(), end.line()),
        }
    }

    fn remove_line_prefix(&self, line: i32, prefix: &str) {
        let b = &self.inner.buffer;
        let Some(mut start) = b.iter_at_line(line) else { return };
        let mut end = start;
        end.forward_chars(prefix.chars().count() as i32);
        b.delete(&mut start, &mut end);
    }

    fn add_line_prefix(&self, line: i32, prefix: &str) {
        let b = &self.inner.buffer;
        let Some(mut start) = b.iter_at_line(line) else { return };
        b.insert(&mut start, prefix);
    }

    /// Turn every selected line into a list of the given kind, or strip the
    /// markers if all of them already are one.
    fn toggle_list(&self, prefixes: &[&'static str]) {
        let (first, last) = self.selected_lines();
        let remove = (first..=last).all(|line| self.line_prefix(line, prefixes).is_some());

        for line in first..=last {
            // a line is only ever one kind of list item, so swapping replaces
            if let Some(existing) = self.line_prefix(line, &LIST_PREFIXES) {
                self.remove_line_prefix(line, existing);
            }
            if !remove {
                self.add_line_prefix(line, prefixes[0]);
            }
        }
    }

    pub fn toggle_checklist(&self) {
        self.toggle_list(&CHECK_PREFIXES);
    }

    pub fn toggle_bullets(&self) {
        self.toggle_list(&BULLET_PREFIXES);
    }

    /// Re-applies the marker styling to every list item in the note.
    fn update_markers(&self) {
        let b = &self.inner.buffer;
        b.remove_tag(&self.inner.tags.marker, &b.start_iter(), &b.end_iter());

        for line in 0..b.line_count() {
            if self.line_prefix(line, &LIST_PREFIXES).is_none() {
                continue;
            }
            let Some(start) = b.iter_at_line(line) else { continue };
            let mut end = start;
            end.forward_char();
            b.apply_tag(&self.inner.tags.marker, &start, &end);
        }
    }

    /// Whether a click at this spot lands on a checkbox rather than on text.
    fn is_checkbox_at(&self, iter: &gtk::TextIter) -> bool {
        iter.line_offset() <= 1 && self.line_prefix(iter.line(), &CHECK_PREFIXES).is_some()
    }

    /// Tick or untick every checkbox in the selection.
    pub fn toggle_checkboxes(&self) {
        let (first, last) = self.selected_lines();
        for line in first..=last {
            self.toggle_checkbox(line);
        }
    }

    /// Tick or untick the checkbox `line` starts with.
    pub fn toggle_checkbox(&self, line: i32) -> bool {
        let Some(prefix) = self.line_prefix(line, &CHECK_PREFIXES) else { return false };
        let b = &self.inner.buffer;
        let Some(mut start) = b.iter_at_line(line) else { return false };
        let mut end = start;
        end.forward_char();

        // swapping one character for another leaves every tag offset in the note
        // untouched
        b.delete(&mut start, &mut end);
        b.insert(&mut start, if prefix.starts_with(UNCHECKED) { CHECKED } else { UNCHECKED });
        true
    }

    // ---- read-only preview -------------------------------------------------

    fn clip_content(content: &str) -> String {
        const MAX_LINES: usize = 5;
        const MAX_CHARS: usize = 240;

        let content = content.trim_end();
        let mut cut: String = content.split('\n').take(MAX_LINES).collect::<Vec<_>>().join("\n");
        if cut.chars().count() > MAX_CHARS {
            cut = cut.chars().take(MAX_CHARS).collect();
        }
        if cut.chars().count() < content.chars().count() {
            format!("{cut}…")
        } else {
            cut
        }
    }

    fn show_content(&self) {
        let Some(note) = self.note() else { return };
        let b = &self.inner.buffer;
        let content = note.content();

        if content.chars().all(char::is_whitespace) {
            b.set_text("");
            let mut start = b.start_iter();
            b.insert_markup(&mut start, &format!("<i>({})</i>", glib::markup_escape_text(&gettext("Empty Note"))));
        } else {
            b.set_text(&Self::clip_content(&content.replace(IMAGE_PLACEHOLDER, "▣")));
            self.clear_tags();
            self.init_tags(&note.tags());
            self.update_markers();
        }
    }

    // ---- editing -----------------------------------------------------------

    fn change_content(&self, content: String) {
        let Some(note) = self.note() else { return };
        note.set_content(content);
        note.touch();
    }

    fn on_content_changed(&self) {
        if self.note().is_none() {
            return;
        }
        self.update_links();
        self.update_markers();
        self.sync_images();
        let content = self.content_text();
        if self.note().is_some_and(|n| n.content() == content) {
            return;
        }
        self.change_content(content);
    }

    /// A newline after a list item carries the list on, or ends it when the item was empty.
    fn on_text_inserted(&self, buffer: &gtk::TextBuffer, loc: &gtk::TextIter, text: &str) {
        if text != "\n" {
            return;
        }
        let mut start_iter = *loc;
        start_iter.backward_char();
        start_iter.set_line_offset(0);

        let mut end_iter = start_iter;
        end_iter.forward_chars(2);
        let chars = buffer.text(&start_iter, &end_iter, false).to_string();

        let is_simple = {
            let mut c = chars.chars();
            matches!(
                (c.next(), c.next(), c.next()),
                (Some('-' | '+' | '*' | '•' | '☐' | '☑'), Some(' '), None)
            )
        };

        if is_simple {
            // a ticked item carries on as a fresh, unticked one
            let first = chars.chars().next().unwrap();
            let bullet = if first == '☑' { '☐' } else { first };
            let mut line_end = *loc;
            line_end.backward_char();
            if line_end.line_offset() == 2 {
                start_iter.set_line_offset(0);
                let mut at = *loc;
                buffer.delete(&mut start_iter, &mut at);
            } else {
                let mut at = *loc;
                buffer.insert(&mut at, &format!("{bullet} "));
            }
            return;
        }

        let limit = {
            let mut l = start_iter;
            l.forward_chars(10);
            l
        };
        let mut search_end = start_iter;
        search_end.forward_find_char(|c| c == ' ', Some(&limit));
        search_end.forward_char();
        let chars = buffer.text(&start_iter, &search_end, false).to_string();

        let Some(digits) = chars.strip_suffix(". ").filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) else {
            return;
        };
        let Ok(number) = digits.parse::<u64>() else { return };
        let mut line_end = *loc;
        line_end.backward_char();
        if line_end.line_offset() as usize == number.to_string().len() + 2 {
            start_iter.set_line_offset(0);
            let mut at = *loc;
            buffer.delete(&mut start_iter, &mut at);
        } else {
            let mut at = *loc;
            buffer.insert(&mut at, &format!("{}. ", number + 1));
        }
    }

    // ---- images ------------------------------------------------------------

    /// Puts the images of the note back where its content has placeholders.
    fn restore_images(&self, note: &NoteObject) {
        let b = &self.inner.buffer;
        self.inner.anchors.borrow_mut().clear();
        let images = note.images();
        if images.is_empty() {
            return;
        }

        self.inner.restoring.set(true);
        for NoteImage { offset, file } in images {
            let mut at = b.iter_at_offset(offset as i32);
            if at.char() != IMAGE_PLACEHOLDER {
                continue;
            }
            let mut end = at;
            end.forward_char();
            b.delete(&mut at, &mut end);

            let mut at = b.iter_at_offset(offset as i32);
            let anchor = b.create_child_anchor(&mut at);
            self.inner.anchors.borrow_mut().insert(anchor, file);
        }
        self.inner.restoring.set(false);
    }

    /// The images the buffer holds right now, as they are saved in the note.
    fn collect_images(&self) -> Vec<NoteImage> {
        let b = &self.inner.buffer;
        self.inner.anchors.borrow_mut().retain(|a, _| !a.is_deleted());
        if self.inner.anchors.borrow().is_empty() {
            return vec![];
        }

        let mut images = vec![];
        let mut at = b.start_iter();
        loop {
            if let Some(anchor) = at.child_anchor() {
                if let Some(file) = self.inner.anchors.borrow().get(&anchor) {
                    images.push(NoteImage { offset: at.offset() as u32, file: file.clone() });
                }
            }
            if !at.forward_char() {
                break;
            }
        }
        images
    }

    fn sync_images(&self) {
        let Some(note) = self.note() else { return };
        let images = self.collect_images();
        if images != note.images() {
            note.set_images(images);
        }
    }

    /// Puts the image `file` in the note, at the cursor unless told where.
    fn insert_image(&self, file: String, at: Option<gtk::TextIter>) {
        let b = &self.inner.buffer;
        let mut iter = at.unwrap_or_else(|| b.iter_at_mark(&b.get_insert()));

        // the change is handled once the anchor is known, or its image would be
        // left out of the note until the next edit
        self.inner.restoring.set(true);
        let anchor = b.create_child_anchor(&mut iter);
        self.inner.anchors.borrow_mut().insert(anchor.clone(), file.clone());
        self.inner.restoring.set(false);
        self.on_content_changed();

        let views: Vec<_> = self.inner.image_views.borrow().iter().filter_map(|(v, _)| v.upgrade()).collect();
        for view in views {
            self.add_picture(&view, &anchor, &file);
        }
    }

    fn insert_textures(&self, textures: &[gdk::Texture], at: Option<gtk::TextIter>) {
        for texture in textures {
            if let Some(file) = save_texture(texture) {
                self.insert_image(file, at);
            }
        }
    }

    fn add_picture(&self, view: &gtk::TextView, anchor: &gtk::TextChildAnchor, file: &str) {
        let texture = load_texture(file);
        let widget: gtk::Widget = match &texture {
            Some(texture) => gtk::Picture::builder()
                .paintable(texture)
                .can_shrink(true)
                .content_fit(gtk::ContentFit::ScaleDown)
                .halign(gtk::Align::Start)
                .build()
                .upcast(),
            None => gtk::Label::new(Some(&format!("({})", gettext("Image not found")))).upcast(),
        };
        widget.add_css_class("note-image");
        if let Some(texture) = &texture {
            unsafe { widget.set_data("natural-size", (texture.width(), texture.height())) };
        }

        view.add_child_at_anchor(&widget, anchor);
        fit_picture(view, &widget);
        for (v, widgets) in self.inner.image_views.borrow_mut().iter_mut() {
            if v.upgrade().as_ref() == Some(view) {
                widgets.insert(anchor.clone(), widget.clone());
            }
        }
    }

    /// Shows the images in `view`, a view of this note's buffer, and lets
    /// images be pasted or dropped into it.
    ///
    /// The buffer is shared but widgets belong to one text view, so this is
    /// done on the view the user works in.
    pub fn attach_images(&self, view: &gtk::TextView) {
        self.inner.image_views.borrow_mut().push((view.downgrade(), HashMap::new()));

        let existing: Vec<_> = self.inner.anchors.borrow().iter().map(|(a, f)| (a.clone(), f.clone())).collect();
        for (anchor, file) in existing {
            if !anchor.is_deleted() {
                self.add_picture(view, &anchor, &file);
            }
        }

        // an image never grows wider than the note
        let last_width = Cell::new(-1);
        let weak = Rc::downgrade(&self.inner);
        view.add_tick_callback(move |view, _| {
            let width = view.width();
            if width == last_width.get() {
                return glib::ControlFlow::Continue;
            }
            last_width.set(width);
            if let Some(inner) = weak.upgrade() {
                let widgets: Vec<_> = inner
                    .image_views
                    .borrow()
                    .iter()
                    .filter(|(v, _)| v.upgrade().as_ref() == Some(view))
                    .flat_map(|(_, w)| w.values().cloned().collect::<Vec<_>>())
                    .collect();
                for widget in widgets {
                    fit_picture(view, &widget);
                }
            }
            glib::ControlFlow::Continue
        });

        let nb = self.clone();
        view.connect_paste_clipboard(move |view| nb.paste_images(view));

        let drop = gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY);
        drop.set_types(&[gdk::FileList::static_type(), gdk::Texture::static_type()]);
        let nb = self.clone();
        drop.connect_drop(clone!(
            #[weak]
            view,
            #[upgrade_or]
            false,
            move |_, value, x, y| {
                let textures = if let Ok(t) = value.get::<gdk::Texture>() {
                    vec![t]
                } else if let Ok(list) = value.get::<gdk::FileList>() {
                    textures_from_files(&list.files())
                } else {
                    vec![]
                };
                if textures.is_empty() {
                    return false;
                }
                let (bx, by) = view.window_to_buffer_coords(gtk::TextWindowType::Text, x as i32, y as i32);
                nb.insert_textures(&textures, view.iter_at_location(bx, by));
                true
            }
        ));
        view.add_controller(drop);
    }

    fn paste_images(&self, view: &gtk::TextView) {
        let clipboard = view.clipboard();
        let kind = clipboard_has_image(&clipboard);
        if !kind.texture && !kind.files {
            return;
        }

        // whether it holds images is only known once it is read, so the default
        // paste is always replaced, and puts text back when there are no images
        view.stop_signal_emission_by_name("paste-clipboard");

        let nb = self.clone();
        glib::spawn_future_local(async move {
            let textures = read_clipboard_textures(&clipboard).await;
            if !textures.is_empty() {
                nb.insert_textures(&textures, None);
            } else if kind.files {
                if let Ok(Some(text)) = clipboard.read_text_future().await {
                    nb.inner.buffer.insert_at_cursor(&text);
                }
            }
        });
    }

    // ---- checkboxes ----------------------------------------------------------

    /// Lets a checkbox be ticked by clicking the box itself, and points the
    /// cursor at the boxes so they read as something to click.
    pub fn attach_checkbox_gesture(&self, view: &gtk::TextView) {
        let gesture = gtk::GestureClick::new();
        let nb = self.clone();
        gesture.connect_released(clone!(
            #[weak]
            view,
            move |_, _, x, y| {
                // a click that ended a selection was a drag, not a tick
                if nb.inner.buffer.has_selection() {
                    return;
                }
                let (bx, by) = view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
                // only the box toggles, so the rest of the line stays editable
                if let Some(iter) = view.iter_at_location(bx, by) {
                    if nb.is_checkbox_at(&iter) {
                        nb.toggle_checkbox(iter.line());
                    }
                }
            }
        ));
        view.add_controller(gesture);

        let motion = gtk::EventControllerMotion::new();
        let nb = self.clone();
        motion.connect_motion(clone!(
            #[weak]
            view,
            move |_, x, y| {
                let (bx, by) = view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
                let on_box = view.iter_at_location(bx, by).is_some_and(|i| nb.is_checkbox_at(&i));
                view.set_cursor_from_name(Some(if on_box { "pointer" } else { "text" }));
            }
        ));
        motion.connect_leave(clone!(
            #[weak]
            view,
            move |_| view.set_cursor_from_name(Some("text"))
        ));
        view.add_controller(motion);
    }
}

/// A text view gives an anchored widget its minimum size, which for a picture
/// that can shrink is nothing, so the size is set here: the picture's own, or
/// less when the note is narrower.
fn fit_picture(view: &gtk::TextView, widget: &gtk::Widget) {
    let Some((width, height)) = (unsafe { widget.data::<(i32, i32)>("natural-size").map(|p| *p.as_ref()) }) else {
        return;
    };
    let room = view.width() - view.left_margin() - view.right_margin();
    let fitted = if room > 0 { width.min(room).max(32) } else { width };
    widget.set_width_request(fitted);
    widget.set_height_request(((height as f64 * fitted as f64 / width as f64).round() as i32).max(1));
}

fn compare_tags(a: &[Tag], b: &[Tag]) -> bool {
    a.len() == b.len() && a.iter().all(|t| b.contains(t))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_clipped_to_five_lines() {
        assert_eq!(NoteBuffer::clip_content("a\nb\n"), "a\nb");
        assert_eq!(NoteBuffer::clip_content("1\n2\n3\n4\n5\n6"), "1\n2\n3\n4\n5…");
    }

    #[test]
    fn preview_is_clipped_to_240_characters() {
        let long = "x".repeat(300);
        let clipped = NoteBuffer::clip_content(&long);
        assert_eq!(clipped.chars().count(), 241);
        assert!(clipped.ends_with('…'));
    }

    #[test]
    fn text_sizes_round_trip_through_their_names() {
        for size in [TextSize::Normal, TextSize::Small, TextSize::Large, TextSize::Larger] {
            assert_eq!(TextSize::from_name(size.name()), size);
        }
        assert_eq!(TextSize::from_name("nonsense"), TextSize::Normal);
    }
}

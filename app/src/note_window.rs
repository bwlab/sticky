use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use sticky_core::{links, Style, NO_POSITION};

use crate::application::Application;
use crate::note_object::NoteObject;
use crate::style_selector::StyleSelector;
use crate::util::{confirm_delete, set_style_class};
use crate::view::{NoteBuffer, TextSize};

/// Formats that are only reachable from the formatting menu. They keep their
/// state so the menu item can show whether they are on, in place of the
/// toolbar button the other formats have.
const MENU_FORMATS: [&str; 3] = ["monospace", "highlight", "header"];

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/com/vixalien/sticky/ui/window.ui")]
    pub struct NoteWindow {
        #[template_child]
        pub container: TemplateChild<adw::ToolbarView>,
        #[template_child]
        pub text: TemplateChild<gtk::TextView>,
        #[template_child]
        pub menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub title_label: TemplateChild<gtk::EditableLabel>,
        #[template_child]
        pub bold_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub underline_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub italic_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub strikethrough_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub color_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub action_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub action_revealer: TemplateChild<gtk::Revealer>,

        pub note: RefCell<Option<NoteObject>>,
        pub view: NoteBuffer,
        pub deleted: Cell<bool>,
        pub cursor_scroll_source: RefCell<Option<glib::SourceId>>,
        pub last_revealer: Cell<bool>,
        pub on_deleted: RefCell<Option<Rc<dyn Fn(&str)>>>,
    }

    impl Default for NoteWindow {
        fn default() -> Self {
            Self {
                container: Default::default(),
                text: Default::default(),
                menu_button: Default::default(),
                title_label: Default::default(),
                bold_button: Default::default(),
                underline_button: Default::default(),
                italic_button: Default::default(),
                strikethrough_button: Default::default(),
                color_button: Default::default(),
                action_button: Default::default(),
                action_revealer: Default::default(),
                note: Default::default(),
                view: NoteBuffer::new(true),
                deleted: Default::default(),
                cursor_scroll_source: Default::default(),
                last_revealer: Default::default(),
                on_deleted: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NoteWindow {
        const NAME: &'static str = "StickyNoteWindow";
        type Type = super::NoteWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for NoteWindow {}
    impl WidgetImpl for NoteWindow {}
    impl WindowImpl for NoteWindow {}
    impl ApplicationWindowImpl for NoteWindow {}
    impl AdwApplicationWindowImpl for NoteWindow {}
}

glib::wrapper! {
    pub struct NoteWindow(ObjectSubclass<imp::NoteWindow>) @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl NoteWindow {
    pub fn new(app: &adw::Application, note: &NoteObject) -> NoteWindow {
        let window: NoteWindow = glib::Object::builder().property("application", app).build();
        window.setup(note);
        window
    }

    pub fn note(&self) -> NoteObject {
        self.imp().note.borrow().clone().expect("a note window always has its note")
    }

    pub fn on_deleted(&self, f: impl Fn(&str) + 'static) {
        *self.imp().on_deleted.borrow_mut() = Some(Rc::new(f));
    }

    fn setup(&self, note: &NoteObject) {
        let imp = self.imp();
        *imp.note.borrow_mut() = Some(note.clone());

        self.set_default_width(note.width());
        self.set_default_height(note.height());

        note.bind_property("display-title", self, "title")
            .sync_create()
            .transform_to(|_, title: String| Some(if title.is_empty() { gettext("Sticky Note") } else { title }))
            .build();

        // the label shows the same title as the window, so that a note that
        // hasn't been given one still has something to click on to give it one
        self.bind_property("title", &*imp.title_label, "text").sync_create().build();

        imp.title_label.connect_editing_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |label| {
                if !label.is_editing() {
                    window.rename(&label.text());
                }
            }
        ));

        self.connect_close_request(glib::clone!(
            #[weak]
            note,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |window| {
                window.cancel_cursor_scroll();
                if window.imp().deleted.get() {
                    return glib::Propagation::Proceed;
                }

                // a note that was given a title is worth keeping even while it is empty
                if note.content().trim().is_empty() && note.title().is_empty() {
                    if let Some(app) = window.application().and_downcast::<Application>() {
                        app.delete_note(&note.uuid());
                    }
                    return glib::Propagation::Proceed;
                }

                if window.width() != note.width() {
                    note.set_width(window.width());
                }
                if window.height() != note.height() {
                    note.set_height(window.height());
                }
                glib::Propagation::Proceed
            }
        ));
        self.connect_unrealize(|w| w.cancel_cursor_scroll());

        self.set_style(note.style_enum(), true);

        let view = &imp.view;
        view.attach_view(&imp.text);
        imp.text.set_buffer(Some(view.buffer()));
        view.set_note(note);

        let weak = self.downgrade();
        view.on_selection_changed({
            let weak = weak.clone();
            move || {
                if let Some(w) = weak.upgrade() {
                    w.check_tags();
                }
            }
        });
        view.on_tag_toggle({
            let weak = weak.clone();
            move |tag, active| {
                if let Some(w) = weak.upgrade() {
                    let imp = w.imp();
                    let button = match tag {
                        "bold" => Some(&*imp.bold_button),
                        "underline" => Some(&*imp.underline_button),
                        "italic" => Some(&*imp.italic_button),
                        "strikethrough" => Some(&*imp.strikethrough_button),
                        _ => None,
                    };
                    if let Some(b) = button {
                        b.set_active(active);
                    }
                }
            }
        });
        view.on_link_selected({
            let weak = weak.clone();
            move |text| {
                if let Some(w) = weak.upgrade() {
                    w.update_link(text);
                }
            }
        });

        let queue = glib::clone!(
            #[weak(rename_to = window)]
            self,
            move || window.queue_cursor_scroll()
        );
        view.buffer().connect_cursor_position_notify({
            let queue = queue.clone();
            move |_| queue()
        });
        view.buffer().connect_changed(move |_| queue());

        view.attach_checkbox_gesture(&imp.text);
        view.attach_images(&imp.text);

        self.add_actions();
        self.apply_font_size();

        // the colors are offered in the main menu and in the formatting bar, and
        // the two selectors follow each other
        let selector = StyleSelector::new(note.style_enum());
        let bar_selector = StyleSelector::new(note.style_enum());
        for (a, b) in [(&selector, &bar_selector), (&bar_selector, &selector)] {
            let other = b.clone();
            let window = self.downgrade();
            a.connect_style_changed(move |style| {
                other.set_style(style);
                if let Some(w) = window.upgrade() {
                    w.set_style(style, false);
                    w.note().set_style(style as i32);
                }
            });
        }

        if let Some(popover) = imp.menu_button.popover().and_downcast::<gtk::PopoverMenu>() {
            popover.add_child(&selector.widget, "notestyleswitcher");
        }
        imp.color_button.set_popover(Some(&gtk::Popover::builder().child(&bar_selector.widget).build()));
    }

    fn queue_cursor_scroll(&self) {
        let imp = self.imp();
        if !self.is_realized() || imp.cursor_scroll_source.borrow().is_some() {
            return;
        }
        let window = self.downgrade();
        let id = glib::idle_add_local_full(glib::Priority::DEFAULT_IDLE, move || {
            if let Some(w) = window.upgrade() {
                *w.imp().cursor_scroll_source.borrow_mut() = None;
                w.scroll_cursor_into_view();
            }
            glib::ControlFlow::Break
        });
        *imp.cursor_scroll_source.borrow_mut() = Some(id);
    }

    fn cancel_cursor_scroll(&self) {
        if let Some(id) = self.imp().cursor_scroll_source.borrow_mut().take() {
            id.remove();
        }
    }

    fn scroll_cursor_into_view(&self) {
        let text = &*self.imp().text;
        let buffer = self.imp().view.buffer();
        let cursor_rect = text.iter_location(&buffer.iter_at_mark(&buffer.get_insert()));
        let visible = text.visible_rect();
        let visible_top = visible.y() + text.top_margin();
        let visible_bottom = visible.y() + visible.height() - text.bottom_margin();

        let offset = if cursor_rect.y() < visible_top {
            cursor_rect.y() - visible_top
        } else if cursor_rect.y() + cursor_rect.height() > visible_bottom {
            cursor_rect.y() + cursor_rect.height() - visible_bottom
        } else {
            0
        };
        if offset == 0 {
            return;
        }
        if let Some(adjustment) = text.vadjustment() {
            adjustment.set_value(adjustment.value() + offset as f64);
        }
    }

    /// The title shown in the header bar and the window manager: the note's own
    /// title when it has one, and one derived from its content otherwise.
    fn display_title(&self) -> String {
        let title = self.note().display_title();
        if title.is_empty() { gettext("Sticky Note") } else { title }
    }

    /// Gives the note the title left in the header bar, or takes its title away
    /// when the label was emptied, letting it follow the content again.
    ///
    /// The label shows the derived title of an untitled note, so text that was
    /// left as it was found means the note keeps having no title of its own.
    fn rename(&self, text: &str) {
        let title = text.trim();
        if title == self.display_title() {
            return;
        }
        let note = self.note();
        note.set_title(title);
        note.touch();
    }

    fn update_link(&self, text: Option<String>) {
        let imp = self.imp();
        let mut selected = false;

        if let Some(text) = text {
            if let Some(link) = links::find(&text).into_iter().next() {
                imp.action_button.set_action_target_value(Some(&link.href.to_variant()));
                // an address opens the mail client, not the browser: say so
                imp.action_button.set_label(&if link.is_email { gettext("Open in Mail") } else { gettext("Open Link") });
                selected = true;
            }
        }

        if imp.last_revealer.get() == selected {
            return;
        }
        imp.last_revealer.set(selected);
        imp.action_revealer.set_reveal_child(selected);

        // wait for the transition to finish
        let window = self.downgrade();
        glib::timeout_add_local_once(Duration::from_millis(imp.action_revealer.transition_duration() as u64), move || {
            let Some(w) = window.upgrade() else { return };
            let imp = w.imp();
            let text = &*imp.text;
            let target = adw::PropertyAnimationTarget::new(text, "bottom-margin");
            let extra = if selected { imp.action_revealer.height() } else { 0 };
            let animation = adw::TimedAnimation::new(
                text,
                text.bottom_margin() as f64,
                (60 + extra) as f64,
                imp.action_revealer.transition_duration(),
                target,
            );
            animation.play();
        });
    }

    fn check_tags(&self) {
        let imp = self.imp();
        for (name, tag) in imp.view.tags().actions() {
            let active = imp.view.has_tag(&tag).is_some();

            let button = match name {
                "bold" => Some(&*imp.bold_button),
                "underline" => Some(&*imp.underline_button),
                "italic" => Some(&*imp.italic_button),
                "strikethrough" => Some(&*imp.strikethrough_button),
                _ => None,
            };
            if let Some(b) = button {
                if b.is_active() != active {
                    b.set_active(active);
                }
            }

            if let Some(action) = self.lookup_action(name).and_downcast::<gio::SimpleAction>() {
                if action.state().and_then(|s| s.get::<bool>()).is_some_and(|s| s != active) {
                    action.set_state(&active.to_variant());
                }
            }
        }

        let size = imp.view.text_size();
        // selection-changed can fire while the buffer is assigned, before add_actions()
        if let Some(action) = self.lookup_action("text-size").and_downcast::<gio::SimpleAction>() {
            if action.state().and_then(|s| s.get::<String>()).as_deref() != Some(size.name()) {
                action.set_state(&size.name().to_variant());
            }
        }
    }

    fn position_variant(&self) -> glib::Variant {
        let note = self.note();
        let has = note.x() != NO_POSITION && note.y() != NO_POSITION;
        (has, if has { note.x() } else { 0 }, if has { note.y() } else { 0 }).to_variant()
    }

    fn add_actions(&self) {
        let note = self.note();
        let imp = self.imp();

        let delete = gio::SimpleAction::new("delete", None);
        delete.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| window.delete()
        ));
        self.add_action(&delete);

        // the two actions below are read by the GNOME Shell extension, which keeps
        // the window above the others (Wayland clients can't do that themselves)
        let keep_above = gio::SimpleAction::new_stateful("keep-above", None, &note.keep_above().to_variant());
        keep_above.connect_change_state(glib::clone!(
            #[weak]
            note,
            move |action, value| {
                let Some(value) = value else { return };
                if let Some(b) = value.get::<bool>() {
                    note.set_keep_above(b);
                    action.set_state(value);
                }
            }
        ));
        self.add_action(&keep_above);

        // (has position, x, y): the extension restores it when the window opens and
        // reports it again whenever the window is moved
        let position = gio::SimpleAction::new_stateful(
            "position",
            Some(&<(bool, i32, i32) as StaticVariantType>::static_variant_type()),
            &self.position_variant(),
        );
        position.connect_change_state(glib::clone!(
            #[weak]
            note,
            move |action, value| {
                let Some(value) = value else { return };
                let Some((has, x, y)) = value.get::<(bool, i32, i32)>() else { return };
                if !has {
                    return;
                }
                {
                    let _guard = note.freeze_notify();
                    note.set_x(x);
                    note.set_y(y);
                }
                action.set_state(value);
            }
        ));
        self.add_action(&position);

        self.add_action(&gio::SimpleAction::new_stateful("note-uuid", None, &note.uuid().to_variant()));

        let set_font_size = gio::SimpleAction::new_stateful("set-font-size", Some(glib::VariantTy::INT32), &note.font_size().to_variant());
        set_font_size.connect_change_state(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |action, value| {
                let Some(value) = value else { return };
                let Some(size) = value.get::<i32>() else { return };
                let note = window.note();
                note.set_font_size(size);
                note.touch();
                window.apply_font_size();
                action.set_state(value);
            }
        ));
        self.add_action(&set_font_size);

        let rename = gio::SimpleAction::new("rename", None);
        rename.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| window.imp().title_label.start_editing()
        ));
        self.add_action(&rename);

        for (name, tag) in imp.view.tags().actions() {
            let action = if MENU_FORMATS.contains(&name) {
                gio::SimpleAction::new_stateful(name, None, &false.to_variant())
            } else {
                gio::SimpleAction::new(name, None)
            };
            action.connect_activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _| {
                    window.imp().view.apply_tag(&tag);
                    window.check_tags();
                }
            ));
            self.add_action(&action);
        }

        let text_size = gio::SimpleAction::new_stateful("text-size", Some(glib::VariantTy::STRING), &"normal".to_variant());
        text_size.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, parameter| {
                let Some(name) = parameter.and_then(|p| p.get::<String>()) else { return };
                window.imp().view.set_text_size(TextSize::from_name(&name));
                window.check_tags();
            }
        ));
        self.add_action(&text_size);

        let checklist = gio::SimpleAction::new("checklist", None);
        checklist.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| window.imp().view.toggle_checklist()
        ));
        self.add_action(&checklist);

        let bullets = gio::SimpleAction::new("bullets", None);
        bullets.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| window.imp().view.toggle_bullets()
        ));
        self.add_action(&bullets);

        let toggle_checkbox = gio::SimpleAction::new("toggle-checkbox", None);
        toggle_checkbox.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| window.imp().view.toggle_checkboxes()
        ));
        self.add_action(&toggle_checkbox);
    }

    fn apply_font_size(&self) {
        let text = &*self.imp().text;
        for class in text.css_classes() {
            if class.starts_with("font-size-") {
                text.remove_css_class(&class);
            }
        }
        text.add_css_class(&format!("font-size-{}", self.note().font_size()));
    }

    fn set_style(&self, style: Style, is_init: bool) {
        set_style_class(&*self.imp().container, style);
        if !is_init {
            self.note().touch();
        }
    }

    fn delete(&self) {
        confirm_delete(self, glib::clone!(
            #[weak(rename_to = window)]
            self,
            move || {
                window.imp().deleted.set(true);
                let f = window.imp().on_deleted.borrow().clone();
                if let Some(f) = f {
                    f(&window.note().uuid());
                }
            }
        ));
    }
}

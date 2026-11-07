use std::cell::{Cell, RefCell};

use chrono::{DateTime, TimeZone, Utc};
use gettextrs::gettext;
use gtk::glib;
use gtk::prelude::*;
use gtk::glib::subclass::prelude::*;
use sticky_core::{derive_title, Note, NoteImage, Style, Tag, NO_POSITION};

use crate::util::{default_style, settings};

mod imp {
    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::NoteObject)]
    pub struct NoteObject {
        #[property(get, set)]
        pub uuid: RefCell<String>,
        #[property(get, set)]
        pub content: RefCell<String>,
        /// The title the note was given, or empty when it follows its content.
        #[property(get, set)]
        pub title: RefCell<String>,
        /// The title as shown: the note's own, or one derived from its content.
        #[property(get, set)]
        pub display_title: RefCell<String>,
        #[property(get, set)]
        pub style: Cell<i32>,
        #[property(get, set)]
        pub font_size: Cell<i32>,
        /// Milliseconds since the epoch.
        #[property(get, set)]
        pub modified: Cell<i64>,
        #[property(get, set)]
        pub width: Cell<i32>,
        #[property(get, set)]
        pub height: Cell<i32>,
        #[property(get, set)]
        pub open: Cell<bool>,
        #[property(get, set, name = "keep-above")]
        pub keep_above: Cell<bool>,
        #[property(get, set)]
        pub x: Cell<i32>,
        #[property(get, set)]
        pub y: Cell<i32>,
        /// Counts the times the tags were replaced: a way to listen for `notify::tag-list`.
        #[property(get, set, name = "tag-list")]
        pub tag_revision: Cell<u32>,
        pub tags: RefCell<Vec<Tag>>,
        pub images: RefCell<Vec<NoteImage>>,
        pub v: Cell<u8>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NoteObject {
        const NAME: &'static str = "NoteObject";
        type Type = super::NoteObject;
    }

    #[glib::derived_properties]
    impl ObjectImpl for NoteObject {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            let update = |o: &super::NoteObject| {
                let shown = derive_display_title(&o.title(), &o.content());
                if shown != o.display_title() {
                    o.set_display_title(shown);
                }
            };
            obj.connect_title_notify(update);
            obj.connect_content_notify(update);
        }
    }
}

glib::wrapper! {
    pub struct NoteObject(ObjectSubclass<imp::NoteObject>);
}

fn derive_display_title(title: &str, content: &str) -> String {
    if title.is_empty() {
        derive_title(content, &gettext("Image"))
    } else {
        title.to_string()
    }
}

fn to_utc(millis: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(millis).single().unwrap_or_else(Utc::now)
}

impl NoteObject {
    pub fn from_note(note: Note) -> NoteObject {
        let note = note.normalized(&gettext("Image"));
        let obj: NoteObject = glib::Object::new();
        let imp = obj.imp();
        imp.v.set(note.v);
        *imp.uuid.borrow_mut() = note.uuid.clone();
        *imp.content.borrow_mut() = note.content.clone();
        *imp.title.borrow_mut() = note.title.clone();
        *imp.display_title.borrow_mut() = derive_display_title(&note.title, &note.content);
        imp.style.set(note.style as i32);
        imp.font_size.set(note.font_size.unwrap_or_else(|| settings().int("font-size")));
        imp.modified.set(note.modified.timestamp_millis());
        imp.width.set(note.width);
        imp.height.set(note.height);
        imp.open.set(note.open);
        imp.keep_above.set(note.keep_above);
        imp.x.set(note.x.unwrap_or(NO_POSITION));
        imp.y.set(note.y.unwrap_or(NO_POSITION));
        *imp.tags.borrow_mut() = note.tags;
        *imp.images.borrow_mut() = note.images;
        obj
    }

    pub fn generate() -> NoteObject {
        NoteObject::from_note(Note::new(default_style(), settings().int("font-size")))
    }

    pub fn to_note(&self) -> Note {
        let imp = self.imp();
        let position = |v: i32| (v != NO_POSITION).then_some(v);
        Note {
            v: imp.v.get().max(1),
            uuid: self.uuid(),
            content: self.content(),
            title: self.title(),
            style: self.style() as u8,
            font_size: Some(self.font_size()),
            tags: imp.tags.borrow().clone(),
            modified: to_utc(self.modified()),
            width: self.width(),
            height: self.height(),
            open: self.open(),
            images: imp.images.borrow().clone(),
            keep_above: self.keep_above(),
            x: position(self.x()),
            y: position(self.y()),
        }
    }

    pub fn style_enum(&self) -> Style {
        Style::from_number(self.style() as i64).unwrap_or(Style::Yellow)
    }

    pub fn tags(&self) -> Vec<Tag> {
        self.imp().tags.borrow().clone()
    }

    pub fn set_tags(&self, tags: Vec<Tag>) {
        *self.imp().tags.borrow_mut() = tags;
        self.set_tag_list(self.tag_list().wrapping_add(1));
    }

    pub fn images(&self) -> Vec<NoteImage> {
        self.imp().images.borrow().clone()
    }

    pub fn set_images(&self, images: Vec<NoteImage>) {
        *self.imp().images.borrow_mut() = images;
    }

    pub fn touch(&self) {
        self.set_modified(Utc::now().timestamp_millis());
    }

    pub fn modified_utc(&self) -> DateTime<Utc> {
        to_utc(self.modified())
    }
}

/// Newest first: the order shared by the all notes view and the tray menu.
pub fn compare_newest_first(a: &NoteObject, b: &NoteObject) -> std::cmp::Ordering {
    b.modified().cmp(&a.modified())
}

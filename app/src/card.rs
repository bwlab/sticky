use std::cell::RefCell;

use chrono::Local;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sticky_core::Style;

use crate::note_object::NoteObject;
use crate::util::{confirm_delete, set_style_class};
use crate::view::NoteBuffer;

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/com/vixalien/sticky/ui/card.ui")]
    pub struct StickyNoteCard {
        #[template_child]
        pub modified_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub title_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub view_image: TemplateChild<gtk::Image>,
        #[template_child]
        pub delete_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub scrolled: TemplateChild<gtk::ScrolledWindow>,

        pub note: RefCell<Option<NoteObject>>,
        pub text_view: gtk::TextView,
        pub buffer: NoteBuffer,
        pub on_deleted: RefCell<Option<Box<dyn Fn(&str)>>>,
    }

    impl Default for StickyNoteCard {
        fn default() -> Self {
            Self {
                modified_label: Default::default(),
                title_label: Default::default(),
                view_image: Default::default(),
                delete_button: Default::default(),
                scrolled: Default::default(),
                note: Default::default(),
                text_view: gtk::TextView::new(),
                buffer: NoteBuffer::new(false),
                on_deleted: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StickyNoteCard {
        const NAME: &'static str = "StickyNoteCard";
        type Type = super::StickyNoteCard;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for StickyNoteCard {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.text_view.set_buffer(Some(self.buffer.buffer()));
            self.text_view.set_editable(false);
            self.text_view.set_cursor_visible(false);
            self.text_view.add_css_class("card-text-view");
            self.text_view.set_wrap_mode(gtk::WrapMode::WordChar);
            self.buffer.attach_view(&self.text_view);
            self.scrolled.set_child(Some(&self.text_view));

            let card = obj.downgrade();
            self.delete_button.connect_clicked(move |_| {
                if let Some(card) = card.upgrade() {
                    card.delete();
                }
            });
        }
    }

    impl WidgetImpl for StickyNoteCard {}
    impl BoxImpl for StickyNoteCard {}
}

glib::wrapper! {
    pub struct StickyNoteCard(ObjectSubclass<imp::StickyNoteCard>) @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl StickyNoteCard {
    pub fn new() -> StickyNoteCard {
        glib::Object::new()
    }

    pub fn set_show_visible_image(&self, visible: bool) {
        self.imp().view_image.set_visible(visible);
    }

    pub fn on_deleted(&self, f: impl Fn(&str) + 'static) {
        *self.imp().on_deleted.borrow_mut() = Some(Box::new(f));
    }

    pub fn uuid(&self) -> Option<String> {
        self.imp().note.borrow().as_ref().map(|n| n.uuid())
    }

    pub fn note(&self) -> Option<NoteObject> {
        self.imp().note.borrow().clone()
    }

    pub fn set_note(&self, note: &NoteObject) {
        let imp = self.imp();
        let changed = imp.note.borrow().as_ref().map(|n| n.style()) != Some(note.style());
        if changed {
            self.set_style(note.style_enum());
        }
        *imp.note.borrow_mut() = Some(note.clone());
        imp.buffer.set_note(note);
        self.update_title_label();
        self.update_modified_label();

        // a text view only knows its height once its lines are laid out, which
        // happens after it is on screen: measure again then
        let text_view = imp.text_view.clone();
        glib::idle_add_local_once(move || text_view.queue_resize());
    }

    /// Stops listening to the note this card was showing.
    pub fn unbind(&self) {
        self.imp().buffer.remove_listeners();
    }

    /// Shows the note's title, but only when it has one of its own: the content
    /// it would otherwise be derived from is right below it in the card.
    pub fn update_title_label(&self) {
        let title = self.imp().note.borrow().as_ref().map(|n| n.title()).unwrap_or_default();
        self.imp().title_label.set_label(&title);
        self.imp().title_label.set_visible(!title.is_empty());
    }

    pub fn update_modified_label(&self) {
        let Some(note) = self.note() else { return };
        let modified = note.modified_utc().with_timezone(&Local);
        // today shows the time, any other day the date
        let format = if Local::now().date_naive() == modified.date_naive() { "%R" } else { "%F" };
        self.imp().modified_label.set_label(&modified.format(format).to_string());
    }

    pub fn set_style(&self, style: Style) {
        set_style_class(self, style);
    }

    fn delete(&self) {
        let Some(uuid) = self.uuid() else { return };
        let card = self.downgrade();
        let parent = self.root().and_then(|r| r.downcast::<gtk::Window>().ok());
        let Some(parent) = parent else { return };
        confirm_delete(&parent, move || {
            if let Some(card) = card.upgrade() {
                if let Some(f) = card.imp().on_deleted.borrow().as_ref() {
                    f(&uuid);
                }
            }
        });
    }
}

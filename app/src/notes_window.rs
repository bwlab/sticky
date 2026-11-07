use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};

use crate::card::StickyNoteCard;
use crate::note_object::{compare_newest_first, NoteObject};
use crate::theme_selector::ThemeSelector;
use crate::util::settings;

type Callback = RefCell<Option<Rc<dyn Fn(&str)>>>;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/com/vixalien/sticky/ui/notes.ui")]
    pub struct NotesWindow {
        #[template_child]
        pub search: TemplateChild<gtk::Box>,
        #[template_child]
        pub notes_box: TemplateChild<gtk::ListView>,
        #[template_child]
        pub no_notes: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub no_results: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,

        pub cards: RefCell<Vec<StickyNoteCard>>,
        pub bindings: RefCell<HashMap<StickyNoteCard, Vec<glib::SignalHandlerId>>>,
        pub sorter: OnceCell<gtk::CustomSorter>,
        pub model: OnceCell<gtk::SingleSelection>,
        pub query: Rc<RefCell<String>>,
        pub old_query: RefCell<String>,
        pub closing: Cell<bool>,
        pub on_note_activated: Callback,
        pub on_deleted: Callback,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NotesWindow {
        const NAME: &'static str = "StickyNotes";
        type Type = super::NotesWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for NotesWindow {}
    impl WidgetImpl for NotesWindow {}
    impl WindowImpl for NotesWindow {}
    impl ApplicationWindowImpl for NotesWindow {}
    impl AdwApplicationWindowImpl for NotesWindow {}
}

glib::wrapper! {
    pub struct NotesWindow(ObjectSubclass<imp::NotesWindow>) @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl NotesWindow {
    pub fn new(app: &adw::Application, notes: &gio::ListStore) -> NotesWindow {
        let window: NotesWindow = glib::Object::builder().property("application", app).build();
        window.setup(notes);
        window
    }

    pub fn on_note_activated(&self, f: impl Fn(&str) + 'static) {
        *self.imp().on_note_activated.borrow_mut() = Some(Rc::new(f));
    }

    pub fn on_deleted(&self, f: impl Fn(&str) + 'static) {
        *self.imp().on_deleted.borrow_mut() = Some(Rc::new(f));
    }

    fn setup(&self, notes: &gio::ListStore) {
        let imp = self.imp();
        let s = settings();

        self.set_default_height(s.int("default-height"));
        self.set_default_width(s.int("default-width"));
        self.connect_close_request(|w| {
            let s = settings();
            let _ = s.set_int("default-height", w.height());
            let _ = s.set_int("default-width", w.width());
            glib::Propagation::Proceed
        });

        // make it transparent
        imp.notes_box.remove_css_class("view");

        let factory = gtk::SignalListItemFactory::new();
        let weak = self.downgrade();
        factory.connect_setup(glib::clone!(
            #[strong]
            weak,
            move |_, item| {
                let Some(window) = weak.upgrade() else { return };
                let item = item.downcast_ref::<gtk::ListItem>().unwrap();
                let card = StickyNoteCard::new();
                let w = window.downgrade();
                card.on_deleted(move |uuid| {
                    if let Some(w) = w.upgrade() {
                        let f = w.imp().on_deleted.borrow().clone();
                        if let Some(f) = f {
                            f(uuid);
                        }
                    }
                });
                window.imp().cards.borrow_mut().push(card.clone());
                item.set_child(Some(&card));
            }
        ));
        factory.connect_bind(glib::clone!(
            #[strong]
            weak,
            move |_, item| {
                let Some(window) = weak.upgrade() else { return };
                let item = item.downcast_ref::<gtk::ListItem>().unwrap();
                let (Some(card), Some(note)) = (
                    item.child().and_downcast::<StickyNoteCard>(),
                    item.item().and_downcast::<NoteObject>(),
                ) else {
                    return;
                };
                card.set_show_visible_image(note.open());
                card.set_note(&note);

                let ids = vec![
                    note.connect_modified_notify(glib::clone!(
                        #[weak]
                        card,
                        #[weak]
                        window,
                        move |_| {
                            card.update_modified_label();
                            if let Some(sorter) = window.imp().sorter.get() {
                                sorter.changed(gtk::SorterChange::Different);
                            }
                        }
                    )),
                    note.connect_title_notify(glib::clone!(
                        #[weak]
                        card,
                        move |_| card.update_title_label()
                    )),
                    note.connect_open_notify(glib::clone!(
                        #[weak]
                        card,
                        move |n| card.set_show_visible_image(n.open())
                    )),
                    note.connect_style_notify(glib::clone!(
                        #[weak]
                        card,
                        move |n| card.set_style(n.style_enum())
                    )),
                ];
                window.imp().bindings.borrow_mut().insert(card, ids);
            }
        ));
        factory.connect_unbind(glib::clone!(
            #[strong]
            weak,
            move |_, item| {
                let Some(window) = weak.upgrade() else { return };
                let item = item.downcast_ref::<gtk::ListItem>().unwrap();
                let (Some(card), Some(note)) = (
                    item.child().and_downcast::<StickyNoteCard>(),
                    item.item().and_downcast::<NoteObject>(),
                ) else {
                    return;
                };
                if let Some(ids) = window.imp().bindings.borrow_mut().remove(&card) {
                    for id in ids {
                        note.disconnect(id);
                    }
                }
                card.unbind();
            }
        ));

        let query = imp.query.clone();
        let filter = gtk::CustomFilter::new(move |obj| {
            let Some(note) = obj.downcast_ref::<NoteObject>() else { return false };
            let query = query.borrow();
            if query.chars().all(char::is_whitespace) {
                return true;
            }
            format!("{}\n{}", note.title(), note.content()).to_lowercase().contains(query.as_str())
        });
        let filter_model = gtk::FilterListModel::new(Some(notes.clone()), Some(filter.clone()));

        let sorter = gtk::CustomSorter::new(|a, b| {
            match (a.downcast_ref::<NoteObject>(), b.downcast_ref::<NoteObject>()) {
                (Some(a), Some(b)) => compare_newest_first(a, b).into(),
                _ => gtk::Ordering::Equal,
            }
        });
        let sorter_model = gtk::SortListModel::new(Some(filter_model), Some(sorter.clone()));
        let _ = imp.sorter.set(sorter);

        let model = gtk::SingleSelection::new(Some(sorter_model));
        model.connect_items_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _, _, _| window.set_status()
        ));
        let _ = imp.model.set(model.clone());

        imp.notes_box.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |list, position| {
                let Some(note) = list.model().and_then(|m| m.item(position)).and_downcast::<NoteObject>() else {
                    return;
                };
                let f = window.imp().on_note_activated.borrow().clone();
                if let Some(f) = f {
                    f(&note.uuid());
                }
            }
        ));

        imp.notes_box.set_factory(Some(&factory));
        imp.notes_box.set_model(Some(&model));

        imp.search_entry.connect_search_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |entry| {
                let imp = window.imp();
                let query = entry.text().to_lowercase();
                let old = imp.old_query.borrow().clone();
                *imp.query.borrow_mut() = query.clone();

                let change = if query.contains(&old) {
                    gtk::FilterChange::MoreStrict
                } else if old.contains(&query) {
                    gtk::FilterChange::LessStrict
                } else {
                    gtk::FilterChange::Different
                };
                filter.changed(change);
                *imp.old_query.borrow_mut() = query;
            }
        ));

        self.set_status();
        self.add_action(&s.create_action("font-size"));

        // Popover menu theme switcher
        if let Some(popover) = imp.menu_button.popover().and_downcast::<gtk::PopoverMenu>() {
            popover.add_child(&ThemeSelector::new(), "themeswitcher");
        }
    }

    pub fn focus_search(&self) {
        self.imp().search_entry.grab_focus();
    }

    fn set_status(&self) {
        let imp = self.imp();
        let n = imp.model.get().map_or(0, |m| m.n_items());
        if n > 0 {
            self.set_visible_child(imp.notes_box.upcast_ref());
        } else if imp.query.borrow().is_empty() {
            self.set_visible_child(imp.no_notes.upcast_ref());
        } else {
            self.set_visible_child(imp.no_results.upcast_ref());
        }
    }

    pub fn set_note_visible(&self, uuid: &str, visible: bool) {
        let card = self.imp().cards.borrow().iter().find(|c| c.uuid().as_deref() == Some(uuid)).cloned();
        if let Some(card) = card {
            card.set_show_visible_image(visible);
        }
    }

    fn set_visible_child(&self, child: &gtk::Widget) {
        let imp = self.imp();
        imp.search.set_visible(child == imp.notes_box.upcast_ref::<gtk::Widget>() || child == imp.no_results.upcast_ref::<gtk::Widget>());
        imp.stack.set_visible_child(child);
    }
}

use std::cell::{Cell, RefCell};
use std::ops::ControlFlow;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use sticky_core::{images as core_images, migration, store::Store, Note};

use crate::images::{data_dir, images_dir};
use crate::note_object::NoteObject;
use crate::note_window::NoteWindow;
use crate::notes_window::NotesWindow;
use crate::preferences::show_preferences;
use crate::tray::{Callbacks, Tray, TrayNote};
use crate::util::{is_cancelled, settings, show_error, APP_ID};

fn store() -> Store {
    Store::new(data_dir())
}

fn image_label() -> String {
    gettext("Image")
}

mod imp {
    use super::*;

    pub struct Application {
        pub notes_list: gio::ListStore,
        pub window: RefCell<Option<NotesWindow>>,
        pub note_windows: RefCell<Vec<NoteWindow>>,
        pub tray: RefCell<Option<Tray>>,
        pub tray_holding: Cell<bool>,
        pub hold_guard: RefCell<Option<gio::ApplicationHoldGuard>>,
        pub open_new_note: Cell<bool>,
        pub show_all_notes: Cell<bool>,
        pub tray_sync: RefCell<Option<glib::SourceId>>,
    }

    impl Default for Application {
        fn default() -> Self {
            Self {
                notes_list: gio::ListStore::new::<NoteObject>(),
                window: Default::default(),
                note_windows: Default::default(),
                tray: Default::default(),
                tray_holding: Default::default(),
                hold_guard: Default::default(),
                open_new_note: Default::default(),
                show_all_notes: Default::default(),
                tray_sync: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Application {
        const NAME: &'static str = "StickyApplication";
        type Type = super::Application;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for Application {
        fn constructed(&self) {
            self.parent_constructed();
            let app = self.obj();
            glib::set_application_name(&gettext("Sticky Notes"));

            app.init_actions();
            app.load_all_notes();

            let flag = |long: &str, short: u8, description: &str| {
                app.add_main_option(long, glib::Char(short as _), glib::OptionFlags::NONE, glib::OptionArg::None, description, None);
            };
            flag("all-notes", b'a', "Show all notes");
            flag("version", b'v', "Print version information and exit");
            flag("new-note", b'n', "Open a new note");
            flag("if-open-note", b'i', "Only show the app if there is atleast one open note");

            println!("Storing Notes at: {}", store().notes_dir().display());
        }
    }

    impl ApplicationImpl for Application {
        fn handle_local_options(&self, options: &glib::VariantDict) -> ControlFlow<glib::ExitCode> {
            let app = self.obj();
            if options.contains("version") {
                println!("{}", env!("CARGO_PKG_VERSION"));
                // quit the invoked process after printing the version number
                // leaving the running instance unaffected
                return ControlFlow::Break(glib::ExitCode::SUCCESS);
            } else if options.contains("all-notes") {
                self.show_all_notes.set(true);
            } else if options.contains("new-note") {
                self.open_new_note.set(true);
            } else if options.contains("if-open-note") {
                let open = app.notes().iter().any(|n| n.open());
                if !open {
                    println!("Sticky Notes not opening because the `-i` flag was passed and there are no open notes");
                    app.quit();
                }
            }
            ControlFlow::Continue(())
        }

        fn startup(&self) {
            self.parent_startup();
            let app = self.obj();
            app.start_tray();
            let apply = || adw::StyleManager::default().set_color_scheme(color_scheme(settings().int("color-scheme")));
            apply();
            settings().connect_changed(Some("color-scheme"), move |_, _| apply());
        }

        fn activate(&self) {
            self.parent_activate();
            let app = self.obj();
            // this is used to track if there is atleast one window open. if there
            // isn't, we show the all_notes
            let mut has_one_open = false;

            if self.show_all_notes.get() {
                app.all_notes();
            } else if self.open_new_note.get() {
                app.new_note();
            } else {
                let s = settings();
                if s.boolean("show-all-notes") || s.boolean("show-list-on-startup") {
                    app.all_notes();
                }

                for note in app.notes() {
                    if note.open() {
                        has_one_open = true;
                        app.show_note(&note.uuid());
                    }
                }

                if !has_one_open {
                    let _ = s.set_boolean("show-all-notes", true);
                    app.all_notes();
                }

                let app = app.clone();
                glib::idle_add_local_once(move || app.offer_import_original());
            }
        }

        fn shutdown(&self) {
            self.obj().save();
            self.parent_shutdown();
        }
    }

    impl GtkApplicationImpl for Application {}
    impl AdwApplicationImpl for Application {}
}

glib::wrapper! {
    pub struct Application(ObjectSubclass<imp::Application>) @extends adw::Application, gtk::Application, gio::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

/// 0 follows the system, 1 is light and 4 is dark, as the settings store them.
fn color_scheme(value: i32) -> adw::ColorScheme {
    match value {
        1 => adw::ColorScheme::ForceLight,
        4 => adw::ColorScheme::ForceDark,
        2 => adw::ColorScheme::PreferLight,
        3 => adw::ColorScheme::PreferDark,
        _ => adw::ColorScheme::Default,
    }
}

impl Application {
    pub fn new() -> Application {
        // set at build time, so that a development build can run next to the stable one
        let id = option_env!("STICKY_APP_ID").unwrap_or(APP_ID);
        glib::Object::builder()
            .property("application-id", id)
            // the app id decides where style.css is looked up: keep it fixed
            .property("resource-base-path", "/com/vixalien/sticky")
            .build()
    }

    pub fn notes(&self) -> Vec<NoteObject> {
        self.imp().notes_list.iter::<NoteObject>().flatten().collect()
    }

    pub fn notes_list(&self) -> gio::ListStore {
        self.imp().notes_list.clone()
    }

    fn load_all_notes(&self) {
        for note in store().load_notes(&image_label()) {
            let note = NoteObject::from_note(note);
            self.save_on_change(&note);
            self.imp().notes_list.append(&note);
        }
        self.sort_notes();

        let app = self.downgrade();
        self.imp().notes_list.connect_items_changed(move |_, _, _, _| {
            if let Some(app) = app.upgrade() {
                app.queue_tray_sync();
            }
        });
    }

    fn sort_notes(&self) {
        self.imp().notes_list.sort(|a, b| {
            let (a, b) = (a.downcast_ref::<NoteObject>().unwrap(), b.downcast_ref::<NoteObject>().unwrap());
            a.modified().cmp(&b.modified())
        });
    }

    // open and keep-above changes don't change modified, but must be saved too
    fn save_on_change(&self, note: &NoteObject) {
        for prop in ["modified", "open", "keep-above", "x", "y"] {
            note.connect_notify_local(Some(prop), |note, _| save_note(note));
        }
        // the tray menu follows the titles and colours of the notes
        let app = self.downgrade();
        for prop in ["modified", "style", "display-title"] {
            let app = app.clone();
            note.connect_notify_local(Some(prop), move |_, _| {
                if let Some(app) = app.upgrade() {
                    app.queue_tray_sync();
                }
            });
        }
    }

    pub fn save(&self) {
        for note in self.notes() {
            save_note(&note);
        }
    }

    fn find_note(&self, uuid: &str) -> Option<NoteObject> {
        self.notes().into_iter().find(|n| n.uuid() == uuid)
    }

    fn find_open_window(&self, uuid: &str) -> Option<NoteWindow> {
        self.imp().note_windows.borrow().iter().find(|w| w.note().uuid() == uuid).cloned()
    }

    fn is_note_open(&self, uuid: &str) -> bool {
        self.find_open_window(uuid).is_some()
    }

    // ---- actions -----------------------------------------------------------

    fn init_actions(&self) {
        let simple = |name: &str, f: fn(&Application)| {
            let action = gio::SimpleAction::new(name, None);
            let app = self.downgrade();
            action.connect_activate(move |_, _| {
                if let Some(app) = app.upgrade() {
                    f(&app);
                }
            });
            self.add_action(&action);
        };

        simple("quit", |a| a.quit());
        simple("about", |a| a.show_about());
        simple("preferences", |a| show_preferences(a.active_window().as_ref()));
        simple("new-note", |a| a.new_note());
        simple("all-notes", |a| {
            let a = a.clone();
            glib::idle_add_local_once(move || a.all_notes());
        });
        simple("cycle", |a| a.cycle(false));
        simple("cycle-reverse", |a| a.cycle(true));
        simple("save", |a| a.save());
        simple("export", |a| a.export_notes());
        simple("import-original", |a| a.import_original());
        simple("import", |a| a.import_notes());

        let open_link = gio::SimpleAction::new("open-link", Some(glib::VariantTy::STRING));
        let app = self.downgrade();
        open_link.connect_activate(move |_, parameter| {
            let (Some(app), Some(uri)) = (app.upgrade(), parameter.and_then(|p| p.get::<String>())) else { return };
            if uri == "null" {
                return;
            }
            gtk::UriLauncher::new(&uri).launch(app.active_window().as_ref(), gio::Cancellable::NONE, |result| {
                if let Err(e) = result {
                    eprintln!("Could not open the link: {e}");
                }
            });
        });
        self.add_action(&open_link);

        self.add_action(&settings().create_action("color-scheme"));

        let accels: &[(&str, &[&str])] = &[
            ("app.quit", &["<Primary>q"]),
            ("app.new-note", &["<Primary>n"]),
            ("app.all-notes", &["<Primary>h"]),
            ("app.save", &["<Primary>s"]),
            ("win.delete", &["<Primary>d"]),
            ("win.rename", &["F2"]),
            ("win.open-primary-menu", &["F10"]),
            ("win.show-help-overlay", &["<Primary>question"]),
            ("window.close", &["<Primary>w"]),
            ("win.bold", &["<Primary>b"]),
            ("win.italic", &["<Primary>i"]),
            ("win.underline", &["<Primary>u"]),
            ("win.strikethrough", &["<Primary>t"]),
            ("win.monospace", &["<Primary>m"]),
            ("win.highlight", &["<Primary>g"]),
            // <Primary>h already shows all notes
            ("win.header", &["<Primary><Shift>h"]),
            ("win.checklist", &["<Primary>e"]),
            ("win.bullets", &["<Primary>l"]),
            ("win.toggle-checkbox", &["<Primary>Return"]),
            ("win.text-size::small", &["<Primary>2"]),
            ("win.text-size::normal", &["<Primary>3"]),
            ("win.text-size::large", &["<Primary>4"]),
            ("win.text-size::larger", &["<Primary>5"]),
        ];
        for (action, keys) in accels {
            self.set_accels_for_action(action, keys);
        }
    }

    fn new_controller(&self) -> gtk::ShortcutController {
        let controller = gtk::ShortcutController::new();
        let add = |reverse: bool, accels: &[&str]| {
            let app = self.downgrade();
            let action = gtk::CallbackAction::new(move |_, _| {
                if let Some(app) = app.upgrade() {
                    app.cycle(reverse);
                }
                glib::Propagation::Stop
            });
            controller.add_shortcut(gtk::Shortcut::new(gtk::ShortcutTrigger::parse_string(&accels.join("|")), Some(action)));
        };
        add(false, &["<Primary>Page_Down", "<Primary>Tab", "<Primary>ISO_Left_Tab", "<Primary>KP_Tab"]);
        add(true, &["<Primary>Page_Up", "<Primary><Shift>Tab", "<Primary><Shift>ISO_Left_Tab", "<Primary><Shift>KP_Tab"]);
        controller
    }

    fn cycle(&self, reverse: bool) {
        self.sort_notes();
        let open: Vec<NoteObject> = self.notes().into_iter().filter(|n| self.is_note_open(&n.uuid())).collect();
        if self.imp().notes_list.n_items() == 0 {
            return;
        }

        let presented = self.active_window();
        if presented.as_ref().is_some_and(|w| w.is::<NotesWindow>()) {
            let pick = if reverse { open.last() } else { open.first() };
            if let Some(w) = pick.and_then(|n| self.find_open_window(&n.uuid())) {
                w.present();
            }
        } else if let Some(current) = presented.and_downcast::<NoteWindow>() {
            let uuid = current.note().uuid();
            let position = open.iter().position(|n| n.uuid() == uuid).unwrap_or(open.len()) as i64;
            let focus = position + if reverse { -1 } else { 1 };
            if focus < 0 || focus >= open.len() as i64 {
                self.all_notes();
            } else if let Some(w) = self.find_open_window(&open[focus as usize].uuid()) {
                w.present();
            }
        }
    }

    fn show_about(&self) {
        let about = adw::AboutDialog::builder()
            .application_name(gettext("Sticky Notes+"))
            .application_icon(self.application_id().unwrap_or_default())
            .developer_name("bwlab")
            .version(env!("CARGO_PKG_VERSION"))
            .developers(["bwlab <info@bwlab.it>", "Angelo Verlain <hey@vixalien.com>", "Christopher Davis <christopherdavis@gnome.org>"])
            .designers(["David Lapshin"])
            // TRANSLATORS: eg. 'Translator Name <your.email@domain.com>' or 'Translator Name https://website.example'
            .translator_credits(gettext("translator-credits"))
            .copyright("© 2023 Angelo Verlain, Christopher Davis, © 2026 bwlab")
            .license_type(gtk::License::MitX11)
            .website("https://github.com/bwlab/sticky")
            .issue_url("https://github.com/bwlab/sticky/issues")
            .build();
        about.present(self.active_window().as_ref());
    }

    // ---- notes and windows ---------------------------------------------------

    pub fn delete_note(&self, uuid: &str) {
        let list = &self.imp().notes_list;
        if let Some(position) = self.notes().iter().position(|n| n.uuid() == uuid) {
            list.remove(position as u32);
        }
        if let Some(window) = self.find_open_window(uuid) {
            window.close();
        }
        store().delete_note(uuid);
    }

    pub fn all_notes(&self) {
        let imp = self.imp();
        if imp.window.borrow().is_none() {
            let window = NotesWindow::new(self.upcast_ref(), &imp.notes_list);

            let app = self.downgrade();
            window.on_note_activated(move |uuid| {
                if let Some(app) = app.upgrade() {
                    app.show_note(uuid);
                }
            });
            let app = self.downgrade();
            window.on_deleted(move |uuid| {
                if let Some(app) = app.upgrade() {
                    app.delete_note(uuid);
                }
            });
            let app = self.downgrade();
            window.connect_close_request(move |_| {
                if let Some(app) = app.upgrade() {
                    *app.imp().window.borrow_mut() = None;
                    let _ = settings().set_boolean("show-all-notes", false);
                }
                glib::Propagation::Proceed
            });
            window.add_controller(self.new_controller());
            *imp.window.borrow_mut() = Some(window);
        }

        let _ = settings().set_boolean("show-all-notes", true);
        if let Some(w) = imp.window.borrow().as_ref() {
            w.present();
        }
    }

    pub fn search_notes(&self) {
        self.all_notes();
        if let Some(w) = self.imp().window.borrow().as_ref() {
            w.focus_search();
        }
    }

    pub fn new_note(&self) {
        let note = NoteObject::generate();
        self.save_on_change(&note);
        self.imp().notes_list.append(&note);
        self.show_note(&note.uuid());
    }

    pub fn show_note(&self, uuid: &str) {
        let Some(note) = self.find_note(uuid) else { return };

        if let Some(window) = self.find_open_window(uuid) {
            window.present();
            return;
        }

        let window = NoteWindow::new(self.upcast_ref(), &note);
        self.imp().note_windows.borrow_mut().push(window.clone());
        window.add_controller(self.new_controller());

        let app = self.downgrade();
        let closing = note.clone();
        window.connect_close_request(move |win| {
            let Some(app) = app.upgrade() else { return glib::Propagation::Proceed };
            let imp = app.imp();
            // if this is the last window, it means that the app will be closed
            // immediately after, so keep this as "open" when the all notes window
            // is open (so that it's restored when the app is opened again)
            if !(imp.note_windows.borrow().len() == 1 && imp.window.borrow().is_none()) {
                closing.set_open(false);
            }
            imp.note_windows.borrow_mut().retain(|w| w != win);
            if let Some(w) = imp.window.borrow().as_ref() {
                w.set_note_visible(&closing.uuid(), false);
            }
            glib::Propagation::Proceed
        });

        let app = self.downgrade();
        window.on_deleted(move |uuid| {
            if let Some(app) = app.upgrade() {
                app.delete_note(uuid);
            }
        });

        if !note.open() {
            note.set_open(true);
        }
        window.present();
    }

    // ---- export and import ---------------------------------------------------

    fn export_notes(&self) {
        let app = self.clone();
        glib::spawn_future_local(async move {
            let dialog = gtk::FileDialog::builder().title(gettext("Export Notes")).initial_name("notes.json").build();
            let parent = app.active_window();
            let file = match dialog.save_future(parent.as_ref()).await {
                Ok(file) => file,
                Err(e) if is_cancelled(&e) => return,
                Err(e) => return show_error(parent.as_ref(), &gettext("Failed to Export Notes"), &e.to_string()),
            };

            let notes: Vec<serde_json::Value> = app
                .notes()
                .iter()
                .map(|n| {
                    let note = n.to_note();
                    let mut value = serde_json::to_value(&note).unwrap_or_default();
                    // the images travel inside the file, so the export is self-contained
                    if !note.images.is_empty() {
                        value["images"] = serde_json::to_value(core_images::export_images(&images_dir(), &note.images)).unwrap_or_default();
                    }
                    value
                })
                .collect();
            let data = serde_json::json!({ "v": 1, "notes": notes });
            let bytes = serde_json::to_vec_pretty(&data).unwrap_or_default();
            if let Err(e) = file.replace_contents(&bytes, None, false, gio::FileCreateFlags::NONE, gio::Cancellable::NONE) {
                eprintln!("Failed to export notes: {e}");
                show_error(app.active_window().as_ref(), &gettext("Failed to Export Notes"), &e.to_string());
            }
        });
    }

    fn import_notes(&self) {
        let app = self.clone();
        glib::spawn_future_local(async move {
            let dialog = gtk::FileDialog::builder().title(gettext("Import Notes")).build();
            let parent = app.active_window();
            let file = match dialog.open_future(parent.as_ref()).await {
                Ok(file) => file,
                Err(e) if is_cancelled(&e) => return,
                Err(e) => return show_error(parent.as_ref(), &gettext("Failed to Import Notes"), &e.to_string()),
            };

            let result = (|| -> Result<(), String> {
                let (contents, _) = file.load_contents(gio::Cancellable::NONE).map_err(|e| e.to_string())?;
                let data: serde_json::Value = serde_json::from_slice(&contents).map_err(|e| e.to_string())?;
                let notes = match data["notes"].as_array() {
                    Some(n) if data["v"] == 1 => n,
                    _ => return Ok(()),
                };

                for value in notes {
                    let mut value = value.clone();
                    let images: Vec<core_images::ExportedImage> =
                        serde_json::from_value(value["images"].take()).unwrap_or_default();
                    let Ok(mut note) = serde_json::from_value::<Note>(value) else { continue };
                    // avoid duplicates by uuid; the uuid becomes a file name, so it is checked
                    if !note.has_safe_uuid() || app.find_note(&note.uuid).is_some() {
                        continue;
                    }
                    note.images = core_images::import_images(&images_dir(), &images);
                    app.add_imported(note);
                }
                app.sort_notes();
                Ok(())
            })();

            if let Err(e) = result {
                eprintln!("Failed to import notes: {e}");
                show_error(app.active_window().as_ref(), &gettext("Failed to Import Notes"), &e);
            }
        });
    }

    fn add_imported(&self, note: Note) {
        let note = NoteObject::from_note(note);
        self.save_on_change(&note);
        self.imp().notes_list.append(&note);
        save_note(&note);
    }

    /// Adds the notes of a folder written by the original app, never touching it.
    fn import_notes_from_folder(&self, folder: &std::path::Path) -> usize {
        let mut imported = 0;
        for note in migration::read_original_notes(folder) {
            if self.find_note(&note.uuid).is_some() {
                continue;
            }
            self.add_imported(note);
            imported += 1;
        }
        self.sort_notes();
        imported
    }

    fn import_original(&self) {
        let app = self.clone();
        glib::spawn_future_local(async move {
            let initial = migration::original_notes_folder(&glib::home_dir());
            let dialog = gtk::FileDialog::builder()
                .title(gettext("Choose the notes folder of the original Sticky Notes"))
                .initial_folder(&gio::File::for_path(initial))
                .build();
            let parent = app.active_window();
            let folder = match dialog.select_folder_future(parent.as_ref()).await {
                Ok(f) => f,
                Err(e) if is_cancelled(&e) => return,
                Err(e) => return eprintln!("Failed to import the original notes: {e}"),
            };
            let Some(path) = folder.path() else { return };

            let imported = app.import_notes_from_folder(&path);
            let dialog = adw::AlertDialog::new(
                Some(&if imported > 0 { gettext("Notes imported") } else { gettext("Nothing to import") }),
                Some(&if imported > 0 {
                    gettext("%d notes imported").replace("%d", &imported.to_string())
                } else {
                    gettext("That folder holds no notes that are not here already.")
                }),
            );
            dialog.add_response("ok", &gettext("OK"));
            dialog.present(app.active_window().as_ref());
        });
    }

    /// First start with no notes: offer to bring over those of the original app.
    fn offer_import_original(&self) {
        if settings().boolean("migration-offered") || self.imp().notes_list.n_items() > 0 {
            return;
        }
        let dialog = adw::AlertDialog::new(
            Some(&gettext("Import your notes?")),
            Some(&gettext("If you used the original Sticky Notes, you can bring its notes here. They are only read, never changed.")),
        );
        dialog.add_responses(&[("no", &gettext("No Thanks")), ("import", &gettext("Import…"))]);
        dialog.set_response_appearance("import", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("import"));
        dialog.set_close_response("no");
        let app = self.clone();
        dialog.connect_response(None, move |_, response| {
            let _ = settings().set_boolean("migration-offered", true);
            if response == "import" {
                app.import_original();
            }
        });
        dialog.present(self.active_window().as_ref());
    }

    // ---- tray ------------------------------------------------------------------

    fn start_tray(&self) {
        let weak = self.downgrade();
        let with_app = move |f: fn(&Application)| {
            let weak = weak.clone();
            move || {
                if let Some(app) = weak.upgrade() {
                    f(&app);
                }
            }
        };
        let show = {
            let weak = self.downgrade();
            move |uuid: &str| {
                if let Some(app) = weak.upgrade() {
                    app.show_note(uuid);
                }
            }
        };
        let registered = {
            let weak = self.downgrade();
            move |registered: bool| {
                let Some(app) = weak.upgrade() else { return };
                let imp = app.imp();
                if registered == imp.tray_holding.get() {
                    return;
                }
                // keep the application running without windows while the tray icon is
                // shown, so that it stays reachable from the tray
                *imp.hold_guard.borrow_mut() = registered.then(|| app.hold());
                imp.tray_holding.set(registered);
            }
        };

        let tray = Tray::new(
            &self.application_id().unwrap_or_default(),
            Callbacks {
                activate: Box::new(with_app(|a| a.all_notes())),
                new_note: Box::new(with_app(|a| a.new_note())),
                show_note: Box::new(show),
                open_app: Box::new(with_app(|a| a.search_notes())),
                quit: Box::new(with_app(|a| a.quit())),
                registered_changed: Box::new(registered),
            },
        );
        *self.imp().tray.borrow_mut() = Some(tray);
        self.sync_tray();
        if let Some(t) = self.imp().tray.borrow().as_ref() {
            t.start();
        }
    }

    fn queue_tray_sync(&self) {
        let imp = self.imp();
        if imp.tray_sync.borrow().is_some() {
            return;
        }
        let app = self.downgrade();
        let id = glib::idle_add_local_once(move || {
            if let Some(app) = app.upgrade() {
                *app.imp().tray_sync.borrow_mut() = None;
                app.sync_tray();
            }
        });
        *imp.tray_sync.borrow_mut() = Some(id);
    }

    fn sync_tray(&self) {
        let mut notes = self.notes();
        notes.sort_by(crate::note_object::compare_newest_first);
        let notes = notes
            .iter()
            .map(|n| TrayNote { uuid: n.uuid(), title: n.display_title(), style: n.style() as u8 })
            .collect();
        if let Some(t) = self.imp().tray.borrow().as_ref() {
            t.set_notes(notes);
        }
    }
}

fn save_note(note: &NoteObject) {
    if let Err(e) = store().save_note(&note.to_note()) {
        eprintln!("Failed to save note {}: {e}", note.uuid());
    }
}

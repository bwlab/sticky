mod application;
mod card;
mod images;
mod note_object;
mod note_window;
mod notes_window;
mod preferences;
mod style_selector;
mod theme_selector;
mod tray;
mod util;
mod view;

use gtk::prelude::*;
use gtk::{gio, glib};

fn main() -> glib::ExitCode {
    let _ = unsafe { gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "") };
    let _ = gettextrs::textdomain(util::GETTEXT_DOMAIN);
    let _ = gettextrs::bind_textdomain_codeset(util::GETTEXT_DOMAIN, "UTF-8");
    if let Some(dir) = option_env!("STICKY_LOCALEDIR") {
        let _ = gettextrs::bindtextdomain(util::GETTEXT_DOMAIN, dir);
    }

    gio::resources_register_include!("sticky.gresource").expect("the compiled resources are part of the binary");
    adw::init().expect("GTK and libadwaita start");

    application::Application::new().run()
}

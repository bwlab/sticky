use adw::prelude::*;
use gettextrs::gettext;
use gtk::gio;

use crate::util::settings;

pub fn show_preferences(parent: Option<&gtk::Window>) {
    let row = adw::SwitchRow::builder()
        .title(gettext("Show All Notes on Startup"))
        .subtitle(gettext("Open the list of all notes together with the open notes"))
        .build();
    settings().bind("show-list-on-startup", &row, "active").flags(gio::SettingsBindFlags::DEFAULT).build();

    let group = adw::PreferencesGroup::builder().title(gettext("Startup")).build();
    group.add(&row);

    let page = adw::PreferencesPage::new();
    page.add(&group);

    let dialog = adw::PreferencesDialog::builder().title(gettext("Preferences")).build();
    dialog.add(&page);
    dialog.present(parent);
}

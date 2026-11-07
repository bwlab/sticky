use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use sticky_core::Style;

pub const APP_ID: &str = "it.bwlab.StickyNotes";
pub const GETTEXT_DOMAIN: &str = "sticky-notes";

thread_local! {
    static SETTINGS: gio::Settings = gio::Settings::new(APP_ID);
}

/// The settings of the app (they live on the main thread, like the windows).
pub fn settings() -> gio::Settings {
    SETTINGS.with(|s| s.clone())
}

pub fn default_style() -> Style {
    Style::from_number(settings().enum_("default-style") as i64).unwrap_or(Style::Yellow)
}

/// The label of a style, for tooltips.
pub fn style_label(style: Style) -> String {
    match style {
        Style::Yellow => gettext("Yellow"),
        Style::Pink => gettext("Pink"),
        Style::Green => gettext("Green"),
        Style::Purple => gettext("Purple"),
        Style::Blue => gettext("Blue"),
        Style::Gray => gettext("Gray"),
        Style::Charcoal => gettext("Charcoal"),
        Style::Window => gettext("Window"),
        Style::Orange => gettext("Orange"),
        Style::Peach => gettext("Peach"),
        Style::Coral => gettext("Coral"),
        Style::Red => gettext("Red"),
        Style::Rose => gettext("Rose"),
        Style::Lavender => gettext("Lavender"),
        Style::Indigo => gettext("Indigo"),
        Style::Cyan => gettext("Cyan"),
        Style::Aqua => gettext("Aqua"),
        Style::Mint => gettext("Mint"),
        Style::Lime => gettext("Lime"),
        Style::Sand => gettext("Sand"),
    }
}

/// Replaces the `style-*` CSS class of a widget with the one for `style`.
pub fn set_style_class(widget: &impl IsA<gtk::Widget>, style: Style) {
    for class in widget.css_classes() {
        if class.starts_with("style-") && class != "style-specifity" {
            widget.remove_css_class(&class);
        }
    }
    widget.add_css_class(&format!("style-{}", style.name()));
}

/// Asks before deleting, unless the user turned that off.
pub fn confirm_delete(parent: &impl IsA<gtk::Widget>, then: impl Fn() + 'static) {
    if !settings().boolean("confirm-delete") {
        then();
        return;
    }
    let dialog = adw::AlertDialog::new(
        Some(&gettext("Delete Note?")),
        Some(&gettext("This action cannot be undone. If you want to hide the note, you can close it instead.")),
    );
    dialog.add_responses(&[("cancel", &gettext("Cancel")), ("delete", &gettext("Delete"))]);
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |_, response| {
        if response == "delete" {
            then();
        }
    });
    dialog.present(Some(parent));
}

pub fn show_error(parent: Option<&gtk::Window>, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_response("ok", &gettext("OK"));
    dialog.present(parent);
}

pub fn is_cancelled(err: &glib::Error) -> bool {
    err.matches(gio::IOErrorEnum::Cancelled) || err.matches(gtk::DialogError::Dismissed)
}

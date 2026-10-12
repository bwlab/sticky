import Adw from "gi://Adw";
import Gio from "gi://Gio";
import Gtk from "gi://Gtk?version=4.0";

import { settings } from "./util.js";

export function show_preferences(parent?: Gtk.Window | null) {
  const row = new Adw.SwitchRow({
    title: _("Show All Notes on Startup"),
    subtitle: _("Open the list of all notes together with the open notes"),
  });

  settings.bind(
    "show-list-on-startup",
    row,
    "active",
    Gio.SettingsBindFlags.DEFAULT,
  );

  const group = new Adw.PreferencesGroup({ title: _("Startup") });
  group.add(row);

  const page = new Adw.PreferencesPage();
  page.add(group);

  const dialog = new Adw.PreferencesDialog({ title: _("Preferences") });
  dialog.add(page);
  dialog.present(parent ?? null);
}

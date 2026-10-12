// Wayland clients can't ask to stay above other windows, so the app exposes the
// choice as the `win.keep-above` action of each note window and this extension
// applies it with Meta.Window.make_above().
import Gio from "gi://Gio";
import { Extension } from "resource:///org/gnome/shell/extensions/extension.js";

const APP_IDS = ["com.vixalien.sticky", "com.vixalien.sticky.Devel"];
const ACTION = "keep-above";

export default class StickyNotesExtras extends Extension {
  enable() {
    this._tracked = new Map();
    this._created = global.display.connect(
      "window-created",
      (_display, window) => this._track(window),
    );
    for (const actor of global.get_window_actors()) {
      this._track(actor.meta_window);
    }
  }

  disable() {
    global.display.disconnect(this._created);
    for (const window of [...this._tracked.keys()]) this._untrack(window);
    this._tracked = null;
  }

  _track(window) {
    if (this._tracked.has(window)) return;
    if (!APP_IDS.includes(window.get_gtk_application_id())) return;

    const entry = { ids: [], group: null, groupIds: [], raised: false };
    this._tracked.set(window, entry);

    // the GTK properties may arrive after the window is created
    const connect_group = () => {
      if (entry.group) return;

      const bus_name = window.get_gtk_unique_bus_name();
      const path = window.get_gtk_window_object_path();
      if (!bus_name || !path) return;

      entry.group = Gio.DBusActionGroup.get(Gio.DBus.session, bus_name, path);
      const sync = () => this._sync(window, entry);
      for (const signal of ["action-added", "action-state-changed"]) {
        entry.groupIds.push(entry.group.connect(signal, sync));
      }
      // listing the actions is what makes the group start fetching them
      entry.group.list_actions();
      sync();
    };

    for (const prop of ["gtk-unique-bus-name", "gtk-window-object-path"]) {
      entry.ids.push(window.connect(`notify::${prop}`, connect_group));
    }
    entry.ids.push(window.connect("unmanaged", () => this._untrack(window)));
    connect_group();
  }

  _sync(window, entry) {
    if (!entry.group.has_action(ACTION)) return;

    const above = entry.group.get_action_state(ACTION)?.get_boolean() ?? false;
    if (above === window.is_above()) return;

    console.log(`[sticky-notes-extras] ${above ? "raising" : "lowering"} ${window.get_title()}`);
    if (above) window.make_above();
    else window.unmake_above();
    entry.raised = above;
  }

  _untrack(window) {
    const entry = this._tracked.get(window);
    if (!entry) return;

    // give the window back the way it was found
    if (entry.raised) window.unmake_above();

    entry.ids.forEach((id) => window.disconnect(id));
    entry.groupIds.forEach((id) => entry.group.disconnect(id));
    this._tracked.delete(window);
  }
}

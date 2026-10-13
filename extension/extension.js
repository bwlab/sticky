// Wayland clients can't ask to stay above other windows, so the app exposes the
// choice as the `win.keep-above` action of each note window and this extension
// applies it with Meta.Window.make_above().
import Gio from "gi://Gio";
import GLib from "gi://GLib";
import { Extension } from "resource:///org/gnome/shell/extensions/extension.js";

const APP_IDS = [
  "it.bwlab.StickyNotes",
  "it.bwlab.StickyNotes.Devel",
  "com.vixalien.sticky",
  "com.vixalien.sticky.Devel",
];
const ACTION = "keep-above";
const POSITION = "position";
// wait for the window to stop moving before reporting where it is
const REPORT_DELAY_MS = 600;

export default class StickyNotesExtras extends Extension {
  enable() {
    console.log("[sticky-notes-extras] enabled");
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

    const entry = {
      ids: [],
      group: null,
      groupIds: [],
      raised: false,
      placed: false,
      report_source: 0,
    };
    this._tracked.set(window, entry);

    // the GTK properties are only filled in after the window is created, so
    // every window is watched until it turns out to be one of ours
    const attach = () => {
      if (entry.group) return;
      if (!APP_IDS.includes(window.get_gtk_application_id())) return;

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

    for (
      const prop of [
        "gtk-application-id",
        "gtk-unique-bus-name",
        "gtk-window-object-path",
      ]
    ) {
      entry.ids.push(window.connect(`notify::${prop}`, attach));
    }
    entry.ids.push(window.connect("unmanaged", () => this._untrack(window)));
    attach();
  }

  _sync(window, entry) {
    this._place(window, entry);

    if (!entry.group.has_action(ACTION)) return;

    const above = entry.group.get_action_state(ACTION)?.get_boolean() ?? false;
    if (above === window.is_above()) return;

    console.log(`[sticky-notes-extras] ${above ? "raising" : "lowering"} ${window.get_title()}`);
    // a pinned note stays in view whichever workspace you are on
    if (above) {
      window.make_above();
      window.stick();
    } else {
      window.unmake_above();
      window.unstick();
    }
    entry.raised = above;
    console.log(`[sticky-notes-extras] ${window.get_title()} is_above=${window.is_above()} on_all_workspaces=${window.is_on_all_workspaces()}`);
  }

  // moves the window back to where the note was last seen, once. This has to
  // wait until the window is shown: before that, mutter still places it itself
  _place(window, entry) {
    if (entry.placed || !entry.group.has_action(POSITION)) return;
    entry.placed = true;

    const place = () => {
      const [has, x, y] = entry.group.get_action_state(POSITION).deepUnpack();
      if (has && this._on_a_monitor(x, y)) {
        console.log(`[sticky-notes-extras] placing ${window.get_title()} at ${x},${y}`);
        window.move_frame(true, x, y);
      }

      // from now on, report where the user leaves it
      entry.ids.push(
        window.connect("position-changed", () => this._report_later(window, entry)),
      );
    };

    if (window.get_compositor_private()?.mapped) {
      place();
      return;
    }

    const id = window.connect("shown", () => {
      window.disconnect(id);
      entry.ids = entry.ids.filter((i) => i !== id);
      place();
    });
    entry.ids.push(id);
  }

  // a saved position may belong to a monitor that is no longer there
  _on_a_monitor(x, y) {
    const display = global.display;
    for (let i = 0; i < display.get_n_monitors(); i++) {
      const monitor = display.get_monitor_geometry(i);
      if (
        x >= monitor.x - 200 && x < monitor.x + monitor.width - 50 &&
        y >= monitor.y && y < monitor.y + monitor.height - 50
      ) return true;
    }
    return false;
  }

  _report_later(window, entry) {
    if (entry.report_source) GLib.source_remove(entry.report_source);

    entry.report_source = GLib.timeout_add(
      GLib.PRIORITY_DEFAULT,
      REPORT_DELAY_MS,
      () => {
        entry.report_source = 0;
        const { x, y } = window.get_frame_rect();
        console.log(`[sticky-notes-extras] reporting ${window.get_title()} at ${x},${y}`);
        entry.group.change_action_state(
          POSITION,
          new GLib.Variant("(bii)", [true, x, y]),
        );
        return GLib.SOURCE_REMOVE;
      },
    );
  }

  _untrack(window) {
    const entry = this._tracked.get(window);
    if (!entry) return;

    if (entry.report_source) GLib.source_remove(entry.report_source);

    // give the window back the way it was found
    if (entry.raised) {
      window.unmake_above();
      window.unstick();
    }

    entry.ids.forEach((id) => window.disconnect(id));
    entry.groupIds.forEach((id) => entry.group.disconnect(id));
    this._tracked.delete(window);
  }
}

import Gio from "gi://Gio";
import GLib from "gi://GLib";
import Gtk from "gi://Gtk?version=4.0";

import { list_files, load_file } from "./store.js";
import type { INote } from "./util.js";

Gio._promisify(
  Gtk.FileDialog.prototype,
  "select_folder",
  "select_folder_finish",
);

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** Where the original Sticky Notes keeps its notes, relative to the home folder. */
const ORIGINAL_NOTES_PATH =
  ".var/app/com.vixalien.sticky/data/com.vixalien.sticky/notes";

export const original_notes_folder = () =>
  Gio.File.new_for_path(
    GLib.build_filenamev([GLib.get_home_dir(), ORIGINAL_NOTES_PATH]),
  );

/**
 * The notes in a folder written by the original app. Anything that isn't a
 * valid note is skipped: the uuid becomes a file name, so it is checked too.
 */
export function read_original_notes(folder: Gio.File): INote[] {
  const notes: INote[] = [];

  for (const file of list_files(folder)) {
    if (!file.get_basename()!.endsWith(".json")) continue;

    try {
      const data = load_file(file);
      if (
        typeof data?.uuid === "string" && UUID.test(data.uuid) &&
        typeof data.content === "string"
      ) {
        notes.push(data as INote);
      }
    } catch (e) {
      console.error(`Skipping ${file.get_path()}: ${e}`);
    }
  }

  return notes;
}

/** Lets the user pick the folder, through the portal so no permission is needed. */
export async function choose_original_folder(parent: Gtk.Window | null) {
  const dialog = new Gtk.FileDialog({
    title: _("Choose the notes folder of the original Sticky Notes"),
    initial_folder: original_notes_folder(),
  });

  return await (dialog as any).select_folder(parent, null) as Gio.File | null;
}

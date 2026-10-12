// Run: XDG_DATA_HOME=$(mktemp -d) GSETTINGS_BACKEND=keyfile GSETTINGS_SCHEMA_DIR=<build>/schemas STORE_JS=<build>/js/store.js gjs -m tests/store.test.js
// Guards against the data-loss bug: an unreadable file must never get notes deleted.
import Gio from "gi://Gio";
import GLib from "gi://GLib";

globalThis.pkg = { name: "com.vixalien.sticky" };
globalThis._ = (s) => s; // gettext stub; GSETTINGS_SCHEMA_DIR must point at the compiled schema
const { load_notes, save_notes, NewNotesDir, delete_note } = await import(
  `file://${GLib.getenv("STORE_JS")}`
);

const assert = (cond, msg) => {
  if (!cond) throw new Error(`FAIL: ${msg}`);
  print(`ok: ${msg}`);
};
const write = (name, text) =>
  NewNotesDir.get_child(name).replace_contents(
    new TextEncoder().encode(text), null, false, Gio.FileCreateFlags.NONE, null,
  );
const exists = (name) => NewNotesDir.get_child(name).query_exists(null);

NewNotesDir.make_directory_with_parents(null);
const good = "11111111-1111-1111-1111-111111111111.json";
const bad = "22222222-2222-2222-2222-222222222222.json";
write(good, JSON.stringify({ v: 1, uuid: good.slice(0, -5), content: "ciao", title: "", style: 1, tags: [], modified: new Date().toISOString(), width: 300, height: 300, open: false }));
write(bad, '{"v":1,"uuid":"corrotto');
write("strano.json", "{}");
write("note.txt", "non e una nota");

const notes = load_notes();
assert(notes.length === 1, "carica solo la nota valida, salta quella corrotta");
save_notes(notes);
assert(exists(bad), "file corrotto non cancellato dal salvataggio");
assert(exists("strano.json"), "file con nome non uuid non cancellato");
assert(exists("note.txt"), "file non json non cancellato");
assert(exists(good), "nota valida ancora presente");
save_notes([]);
assert(exists(good), "salvataggio con lista vuota non cancella nulla");
delete_note(good.slice(0, -5));
assert(!exists(good), "delete_note esplicito cancella");

// Run: XDG_DATA_HOME=$(mktemp -d) XDG_CONFIG_HOME=$(mktemp -d) GSETTINGS_BACKEND=keyfile GSETTINGS_SCHEMA_DIR=<build>/schemas MIGRATION_JS=<build>/js/migration.js gjs -m tests/migration.test.js
// Reading the folder of the original app: valid notes in, everything else out, source untouched.
import Gio from "gi://Gio";
import GLib from "gi://GLib";

globalThis.pkg = { name: "it.bwlab.StickyNotes" };
globalThis._ = (s) => s;
const { read_original_notes } = await import(`file://${GLib.getenv("MIGRATION_JS")}`);

const assert = (cond, msg) => {
  if (!cond) throw new Error(`FAIL: ${msg}`);
  print(`ok: ${msg}`);
};

const dir = Gio.File.new_for_path(GLib.dir_make_tmp("migration-XXXXXX"));
const write = (name, text) =>
  dir.get_child(name).replace_contents(new TextEncoder().encode(text), null, false, Gio.FileCreateFlags.NONE, null);
const note = (uuid, extra = {}) => JSON.stringify({ v: 1, uuid, content: "x", title: "", style: 1, tags: [], modified: new Date().toISOString(), width: 300, height: 300, open: false, ...extra });

const good = "11111111-1111-1111-1111-111111111111";
write(`${good}.json`, note(good));
write("22222222-2222-2222-2222-222222222222.json", '{"v":1,"uuid":"corrotto');
write("evil.json", note("../../escape"));
write("nocontent.json", JSON.stringify({ uuid: "33333333-3333-3333-3333-333333333333" }));
write("readme.txt", "non è una nota");

const before = new TextDecoder().decode(dir.get_child(`${good}.json`).load_contents(null)[1]);
const found = read_original_notes(dir);
assert(found.length === 1 && found[0].uuid === good, "only the valid note is read");
assert(!found.some((n) => n.uuid.includes("..")), "a uuid that is not a uuid is refused");
assert(new TextDecoder().decode(dir.get_child(`${good}.json`).load_contents(null)[1]) === before, "the source folder is left untouched");
assert(read_original_notes(Gio.File.new_for_path(GLib.dir_make_tmp("empty-XXXXXX"))).length === 0, "an empty folder gives nothing");

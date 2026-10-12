// Run: XDG_DATA_HOME=$(mktemp -d) XDG_CONFIG_HOME=$(mktemp -d) GSETTINGS_BACKEND=keyfile GSETTINGS_SCHEMA_DIR=<build>/schemas IMAGES_JS=<build>/js/images.js gjs -m tests/images.test.js
import Gdk from "gi://Gdk?version=4.0";
import GLib from "gi://GLib";

globalThis.pkg = { name: "com.vixalien.sticky" };
globalThis._ = (s) => s;
const { save_texture, load_texture, export_images, import_images, is_image_file_name, ImagesDir } =
  await import(`file://${GLib.getenv("IMAGES_JS")}`);

const assert = (cond, msg) => {
  if (!cond) throw new Error(`FAIL: ${msg}`);
  print(`ok: ${msg}`);
};

const pixels = new Uint8Array(40 * 20 * 4).fill(200);
const texture = Gdk.MemoryTexture.new(40, 20, Gdk.MemoryFormat.R8G8B8A8, new GLib.Bytes(pixels), 160);

const name = save_texture(texture);
assert(is_image_file_name(name), "the file is named after the content");
assert(save_texture(texture) === name, "the same image is stored once");
assert(load_texture(name)?.get_width() === 40, "a stored image loads back");

for (const bad of ["../notes/x.png", "a.png", `${name}/..`, "", 7, null]) {
  assert(!is_image_file_name(bad) && load_texture(bad) === null, `rejects ${JSON.stringify(bad)}`);
}
assert(load_texture("0".repeat(64) + ".png") === null, "a missing image gives null, not an error");

const exported = export_images([{ offset: 3, file: name }, { offset: 9, file: "../etc/passwd" }]);
assert(exported.length === 1 && typeof exported[0].data === "string", "export embeds valid images and skips bad names");

ImagesDir.get_child(name).delete(null);
const imported = import_images(exported);
assert(imported.length === 1 && imported[0].file === name && imported[0].offset === 3, "import restores the image under its content name");
assert(ImagesDir.get_child(name).query_exists(null), "import wrote the file back");

const forged = import_images([{ offset: 1, file: "../../evil.png", data: exported[0].data }]);
assert(is_image_file_name(forged[0].file) && forged[0].file === name, "import never trusts the file name it is given");
assert(import_images([{ offset: 1, data: "!!not base64!!" }, { data: "x" }]).length <= 1, "garbage entries do not break the import");

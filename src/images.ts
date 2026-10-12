import Gdk from "gi://Gdk?version=4.0";
import Gio from "gi://Gio";
import GLib from "gi://GLib";
import GObject from "gi://GObject";

import { ensure_dir, NewNotesDir } from "./store.js";
import type { NoteImage } from "./util.js";

Gio._promisify(
  Gdk.Clipboard.prototype,
  "read_texture_async",
  "read_texture_finish",
);
Gio._promisify(
  Gdk.Clipboard.prototype,
  "read_value_async",
  "read_value_finish",
);

export const ImagesDir = NewNotesDir.get_parent()!.get_child("images");

// images are named after their content, which also keeps the name safe to use
// as a path, whatever a note file or an imported file claims
const FILE_NAME = /^[0-9a-f]{64}\.png$/;

export const is_image_file_name = (name: unknown): name is string =>
  typeof name === "string" && FILE_NAME.test(name);

const name_for = (bytes: GLib.Bytes) =>
  `${GLib.compute_checksum_for_bytes(GLib.ChecksumType.SHA256, bytes)!}.png`;

const write_image = (name: string, bytes: GLib.Bytes) => {
  ensure_dir(ImagesDir);

  const file = ImagesDir.get_child(name);
  // never rewrite an image that is already there
  if (file.query_exists(null)) return;

  file.replace_contents(
    bytes.get_data()!,
    null,
    false,
    Gio.FileCreateFlags.NONE,
    null,
  );
};

/** Stores a texture as a PNG and returns the name of the file. */
export function save_texture(texture: Gdk.Texture) {
  const bytes = texture.save_to_png_bytes();
  const name = name_for(bytes);
  write_image(name, bytes);
  return name;
}

/** Loads an image, or null when the file is missing or isn't an image. */
export function load_texture(name: string) {
  if (!is_image_file_name(name)) return null;

  try {
    return Gdk.Texture.new_from_file(ImagesDir.get_child(name));
  } catch (e) {
    console.error(`Could not load image ${name}: ${e}`);
    return null;
  }
}

const load_texture_from_file = (file: Gio.File) => {
  try {
    const info = file.query_info(
      "standard::content-type",
      Gio.FileQueryInfoFlags.NONE,
      null,
    );
    if (!info.get_content_type()?.startsWith("image/")) return null;

    return Gdk.Texture.new_from_file(file);
  } catch {
    return null;
  }
};

/** The images among the files, ignoring the ones that aren't. */
export const textures_from_files = (files: Gio.File[]) =>
  files.map(load_texture_from_file).filter((t): t is Gdk.Texture => !!t);

/**
 * What pasting would give: a texture on its own is an image, a list of files
 * is images when they are, and anything else is left to the text view.
 */
export function clipboard_has_image(clipboard: Gdk.Clipboard) {
  const formats = clipboard.get_formats();

  return {
    texture: formats.contain_gtype(Gdk.Texture.$gtype) &&
      !formats.contain_gtype(GObject.TYPE_STRING),
    files: formats.contain_gtype(Gdk.FileList.$gtype),
  };
}

export async function read_clipboard_textures(
  clipboard: Gdk.Clipboard,
): Promise<Gdk.Texture[]> {
  const kind = clipboard_has_image(clipboard);

  try {
    if (kind.texture) {
      const texture = await (clipboard as any).read_texture_async(null);
      return texture ? [texture] : [];
    }

    if (kind.files) {
      const value = await (clipboard as any).read_value_async(
        Gdk.FileList.$gtype,
        GLib.PRIORITY_DEFAULT,
        null,
      );
      return textures_from_files(value.get_boxed().get_files());
    }
  } catch (e) {
    console.error(`Could not read images from the clipboard: ${e}`);
  }

  return [];
}

// -- export and import: the exported file carries the images themselves

export interface ExportedImage extends NoteImage {
  data: string;
}

/** The images of a note with their content, so that the export is self-contained. */
export function export_images(images: NoteImage[] = []): ExportedImage[] {
  const exported: ExportedImage[] = [];

  for (const image of images) {
    if (!is_image_file_name(image.file)) continue;

    try {
      const [, contents] = ImagesDir.get_child(image.file).load_contents(null);
      exported.push({ ...image, data: GLib.base64_encode(contents) });
    } catch (e) {
      console.error(`Could not export image ${image.file}: ${e}`);
    }
  }

  return exported;
}

/**
 * Writes the images of an imported note to disk and returns the references
 * to keep in the note. Images that are already there are left alone, and the
 * names are recomputed from the content rather than trusted.
 */
export function import_images(images: Partial<ExportedImage>[] = []): NoteImage[] {
  const imported: NoteImage[] = [];

  for (const image of images) {
    if (typeof image.offset !== "number" || typeof image.data !== "string") {
      continue;
    }

    try {
      const bytes = new GLib.Bytes(GLib.base64_decode(image.data));
      const name = name_for(bytes);
      write_image(name, bytes);
      imported.push({ offset: image.offset, file: name });
    } catch (e) {
      console.error(`Could not import an image: ${e}`);
    }
  }

  return imported;
}

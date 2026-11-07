use std::path::PathBuf;

use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use sticky_core::images;

use crate::util::APP_ID;

pub fn data_dir() -> PathBuf {
    glib::user_data_dir().join(APP_ID)
}

pub fn images_dir() -> PathBuf {
    data_dir().join("images")
}

/// Stores a texture as a PNG and returns the name of the file.
pub fn save_texture(texture: &gdk::Texture) -> Option<String> {
    let bytes = texture.save_to_png_bytes();
    match images::save_png(&images_dir(), &bytes) {
        Ok(name) => Some(name),
        Err(e) => {
            eprintln!("Could not store the image: {e}");
            None
        }
    }
}

/// Loads an image, or `None` when the file is missing or isn't an image.
pub fn load_texture(name: &str) -> Option<gdk::Texture> {
    let bytes = images::load_png(&images_dir(), name)?;
    match gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)) {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("Could not load image {name}: {e}");
            None
        }
    }
}

fn texture_from_file(file: &gio::File) -> Option<gdk::Texture> {
    let info = file.query_info("standard::content-type", gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE).ok()?;
    if !info.content_type()?.starts_with("image/") {
        return None;
    }
    gdk::Texture::from_file(file).ok()
}

/// The images among the files, ignoring the ones that aren't.
pub fn textures_from_files(files: &[gio::File]) -> Vec<gdk::Texture> {
    files.iter().filter_map(texture_from_file).collect()
}

pub struct ClipboardKind {
    pub texture: bool,
    pub files: bool,
}

/// What pasting would give: a texture on its own is an image, a list of files
/// is images when they are, and anything else is left to the text view.
pub fn clipboard_has_image(clipboard: &gdk::Clipboard) -> ClipboardKind {
    let formats = clipboard.formats();
    ClipboardKind {
        texture: formats.contains_type(gdk::Texture::static_type()) && !formats.contains_type(glib::Type::STRING),
        files: formats.contains_type(gdk::FileList::static_type()),
    }
}

pub async fn read_clipboard_textures(clipboard: &gdk::Clipboard) -> Vec<gdk::Texture> {
    let kind = clipboard_has_image(clipboard);

    if kind.texture {
        match clipboard.read_texture_future().await {
            Ok(Some(texture)) => return vec![texture],
            Ok(None) => {}
            Err(e) => eprintln!("Could not read an image from the clipboard: {e}"),
        }
    } else if kind.files {
        match clipboard.read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT).await {
            Ok(value) => {
                if let Ok(list) = value.get::<gdk::FileList>() {
                    return textures_from_files(&list.files());
                }
            }
            Err(e) => eprintln!("Could not read files from the clipboard: {e}"),
        }
    }
    vec![]
}

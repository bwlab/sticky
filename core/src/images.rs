use std::fs;
use std::io;
use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::note::NoteImage;
use crate::store::write_atomic;

/// Images are named after their content, which also keeps the name safe to use
/// as a path, whatever a note file or an imported file claims.
pub fn is_image_file_name(name: &str) -> bool {
    name.len() == 68
        && name.ends_with(".png")
        && name[..64].bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn name_for(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    format!("{hex}.png")
}

/// Stores PNG bytes in `dir` and returns the file name. An image that is
/// already there is never rewritten.
pub fn save_png(dir: &Path, bytes: &[u8]) -> io::Result<String> {
    let name = name_for(bytes);
    let path = dir.join(&name);
    if !path.exists() {
        write_atomic(&path, bytes)?;
    }
    Ok(name)
}

/// The bytes of a stored image, or `None` when the name is bad or the file is missing.
pub fn load_png(dir: &Path, name: &str) -> Option<Vec<u8>> {
    if !is_image_file_name(name) {
        return None;
    }
    fs::read(dir.join(name)).ok()
}

/// An image with its content, so that an exported file is self-contained.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedImage {
    pub offset: u32,
    #[serde(default)]
    pub file: String,
    pub data: String,
}

pub fn export_images(dir: &Path, images: &[NoteImage]) -> Vec<ExportedImage> {
    images
        .iter()
        .filter_map(|image| {
            let bytes = load_png(dir, &image.file)?;
            Some(ExportedImage { offset: image.offset, file: image.file.clone(), data: STANDARD.encode(bytes) })
        })
        .collect()
}

/// Writes the images of an imported note to disk and returns the references to
/// keep in the note. The names are recomputed from the content, never trusted.
pub fn import_images(dir: &Path, images: &[ExportedImage]) -> Vec<NoteImage> {
    images
        .iter()
        .filter_map(|image| {
            let bytes = STANDARD.decode(&image.data).ok()?;
            let file = save_png(dir, &bytes).ok()?;
            Some(NoteImage { offset: image.offset, file })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really an image but bytes are bytes";

    #[test]
    fn named_after_content_and_stored_once() {
        let tmp = tempfile::tempdir().unwrap();
        let name = save_png(tmp.path(), PNG).unwrap();
        assert!(is_image_file_name(&name));
        assert_eq!(save_png(tmp.path(), PNG).unwrap(), name);
        assert_eq!(load_png(tmp.path(), &name).unwrap(), PNG);
    }

    #[test]
    fn bad_names_and_missing_files_give_none() {
        let tmp = tempfile::tempdir().unwrap();
        for bad in ["../x.png", "abc.png", "", &format!("{}.PNG", "0".repeat(64)), &format!("{}.png", "G".repeat(64))] {
            assert!(!is_image_file_name(bad), "{bad}");
            assert!(load_png(tmp.path(), bad).is_none());
        }
        assert!(load_png(tmp.path(), &format!("{}.png", "0".repeat(64))).is_none());
    }

    #[test]
    fn export_import_round_trip_never_trusts_names() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let name = save_png(src.path(), PNG).unwrap();
        let exported = export_images(
            src.path(),
            &[NoteImage { offset: 3, file: name.clone() }, NoteImage { offset: 4, file: "../../etc/passwd".into() }],
        );
        assert_eq!(exported.len(), 1);

        let mut forged = exported.clone();
        forged[0].file = "../../evil.png".into();
        let imported = import_images(dst.path(), &forged);
        assert_eq!(imported, vec![NoteImage { offset: 3, file: name.clone() }]);
        assert!(dst.path().join(&name).exists());

        let garbage = [ExportedImage { offset: 1, file: String::new(), data: "!!not base64!!".into() }];
        assert!(import_images(dst.path(), &garbage).is_empty());
    }
}

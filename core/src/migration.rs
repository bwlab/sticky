use std::fs;
use std::path::{Path, PathBuf};

use crate::note::Note;
use crate::store::read_note_file;

/// Where the original Sticky Notes keeps its notes, relative to the home folder.
const ORIGINAL_NOTES_PATH: &str = ".var/app/com.vixalien.sticky/data/com.vixalien.sticky/notes";

pub fn original_notes_folder(home: &Path) -> PathBuf {
    home.join(ORIGINAL_NOTES_PATH)
}

fn is_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts.iter().zip([8, 4, 4, 4, 12]).all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The notes in a folder written by the original app, which is only read.
/// Anything that isn't a valid note is skipped: the uuid becomes a file name,
/// so it is checked too.
pub fn read_original_notes(folder: &Path) -> Vec<Note> {
    let Ok(entries) = fs::read_dir(folder) else {
        return vec![];
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|p| read_note_file(&p))
        .filter(|n| is_uuid(&n.uuid))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_valid_notes_are_read_and_the_source_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let good = "11111111-1111-1111-1111-111111111111";
        let text = format!(
            r#"{{"v":1,"uuid":"{good}","content":"ciao","title":"","style":1,"tags":[],"modified":"2026-10-13T08:00:00.000Z","width":300,"height":300,"open":false}}"#
        );
        fs::write(dir.path().join(format!("{good}.json")), &text).unwrap();
        fs::write(
            dir.path().join("evil.json"),
            text.replace(good, "../../evil"),
        )
        .unwrap();
        fs::write(dir.path().join("broken.json"), "{").unwrap();

        let found = read_original_notes(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].uuid, good);
        assert_eq!(fs::read_to_string(dir.path().join(format!("{good}.json"))).unwrap(), text);

        let empty = tempfile::tempdir().unwrap();
        assert!(read_original_notes(empty.path()).is_empty());
    }
}

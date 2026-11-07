use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::note::{is_safe_uuid, Note};

/// Where notes live: `<data dir>/notes/<uuid>.json`, images in `<data dir>/images`.
pub struct Store {
    pub dir: PathBuf,
}

/// Writes through a temporary file, so a crash never leaves half a note.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&tmp, path)
}

/// Reads and parses a note file; `None` for anything that isn't a valid note.
pub fn read_note_file(path: &Path) -> Option<Note> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Store {
        Store { dir: dir.into() }
    }

    pub fn notes_dir(&self) -> PathBuf {
        self.dir.join("notes")
    }

    pub fn images_dir(&self) -> PathBuf {
        self.dir.join("images")
    }

    fn index_file(&self) -> PathBuf {
        self.dir.join("notes.json")
    }

    /// All the notes on disk. A file that can't be read is skipped and left
    /// alone: it must never take the other notes down with it.
    pub fn load_notes(&self, image_label: &str) -> Vec<Note> {
        // the oldest format kept every note in one file: split it up
        if let Ok(bytes) = fs::read(self.index_file()) {
            if let Ok(index) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if index["v"] == 1 {
                    let notes: Vec<Note> = index["notes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|n| serde_json::from_value(n.clone()).ok())
                        .filter(Note::has_safe_uuid)
                        .collect();
                    let all_saved = notes.iter().all(|n| self.save_note(n).is_ok());
                    if all_saved {
                        let _ = fs::remove_file(self.index_file());
                    }
                }
            }
        }

        let Ok(entries) = fs::read_dir(self.notes_dir()) else {
            return vec![];
        };

        let mut notes = vec![];
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            match read_note_file(&path) {
                Some(n) if n.has_safe_uuid() => notes.push(n.normalized(image_label)),
                _ => eprintln!("Skipping unreadable note {}", path.display()),
            }
        }
        notes
    }

    /// Only ever writes. Files are removed solely by `delete_note`, so that a
    /// failed load or a stray file can never cause notes to be erased.
    pub fn save_note(&self, note: &Note) -> io::Result<()> {
        if !is_safe_uuid(&note.uuid) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "unsafe note uuid"));
        }
        let json = serde_json::to_string_pretty(note).map_err(io::Error::other)?;
        write_atomic(&self.notes_dir().join(format!("{}.json", note.uuid)), json.as_bytes())
    }

    pub fn delete_note(&self, uuid: &str) {
        if is_safe_uuid(uuid) {
            let _ = fs::remove_file(self.notes_dir().join(format!("{uuid}.json")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Style;

    fn write(store: &Store, name: &str, text: &str) {
        fs::create_dir_all(store.notes_dir()).unwrap();
        fs::write(store.notes_dir().join(name), text).unwrap();
    }

    // Guards against the data-loss bug: an unreadable file must never get notes deleted.
    #[test]
    fn unreadable_files_are_skipped_and_never_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let good = "11111111-1111-1111-1111-111111111111";
        let good_json = format!(
            r#"{{"v":1,"uuid":"{good}","content":"ciao","title":"","style":1,"tags":[],"modified":"2026-10-13T08:00:00.000Z","width":300,"height":300,"open":false}}"#
        );
        write(&store, &format!("{good}.json"), &good_json);
        write(&store, "22222222-2222-2222-2222-222222222222.json", r#"{"v":1,"uuid":"corrotto"#);
        write(&store, "strano.json", "{}");
        write(&store, "note.txt", "non e una nota");

        let notes = store.load_notes("Image");
        assert_eq!(notes.len(), 1);
        for n in &notes {
            store.save_note(n).unwrap();
        }
        for name in ["22222222-2222-2222-2222-222222222222.json", "strano.json", "note.txt"] {
            assert!(store.notes_dir().join(name).exists(), "{name} was deleted");
        }
        assert!(store.notes_dir().join(format!("{good}.json")).exists());

        store.delete_note(good);
        assert!(!store.notes_dir().join(format!("{good}.json")).exists());
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let mut note = Note::new(Style::Pink, 12);
        note.content = "hello".into();
        store.save_note(&note).unwrap();
        let loaded = store.load_notes("Image");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].content, "hello");
        assert_eq!(loaded[0].style(), Style::Pink);
    }

    #[test]
    fn old_single_file_index_is_split() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let id = "33333333-3333-3333-3333-333333333333";
        fs::write(
            tmp.path().join("notes.json"),
            format!(
                r#"{{"v":1,"state":{{"all_notes":true}},"notes":[{{"v":1,"uuid":"{id}","content":"x","title":"","style":2,"tags":[],"modified":"2026-10-13T08:00:00.000Z","width":300,"height":300,"open":false}}]}}"#
            ),
        )
        .unwrap();
        assert_eq!(store.load_notes("Image").len(), 1);
        assert!(!tmp.path().join("notes.json").exists());
        assert!(store.notes_dir().join(format!("{id}.json")).exists());
    }

    #[test]
    fn unsafe_uuid_is_not_written() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let mut note = Note::new(Style::Yellow, 12);
        note.uuid = "../evil".into();
        assert!(store.save_note(&note).is_err());
    }
}

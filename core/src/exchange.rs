use crate::images::ExportedImage;
use crate::note::Note;

/// The notes of an exported file, each with the images that travel inside it.
/// Anything that is not a valid note is skipped: the file comes from outside.
pub fn parse_import(bytes: &[u8]) -> Result<Vec<(Note, Vec<ExportedImage>)>, String> {
    let data: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let Some(notes) = data["notes"].as_array().filter(|_| data["v"] == 1) else {
        return Ok(vec![]);
    };

    Ok(notes
        .iter()
        .filter_map(|value| {
            let mut value = value.as_object()?.clone();
            let images = value.remove("images").and_then(|i| serde_json::from_value(i).ok()).unwrap_or_default();
            let note: Note = serde_json::from_value(value.into()).ok()?;
            // the uuid becomes a file name
            note.has_safe_uuid().then_some((note, images))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_entries_are_skipped_not_fatal() {
        let ok = r#"{"v":1,"uuid":"11111111-1111-1111-1111-111111111111","content":"a","title":"","style":1,"tags":[],"modified":"2026-10-13T08:00:00.000Z","width":300,"height":300,"open":false,"images":[{"offset":0,"data":"AA=="}]}"#;
        let evil = ok.replace("11111111-1111-1111-1111-111111111111", "../evil");
        let file = format!(r#"{{"v":1,"notes":[5,"x",null,[],{{}},{evil},{ok}]}}"#);

        let parsed = parse_import(file.as_bytes()).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].1.len(), 1);
        assert!(parse_import(b"not json").is_err());
        assert!(parse_import(br#"{"v":2,"notes":[]}"#).unwrap().is_empty());
    }
}

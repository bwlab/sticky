use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The character a note's content holds in place of each image.
pub const IMAGE_PLACEHOLDER: char = '\u{FFFC}';

/// x and y of a note whose window position was never saved.
pub const NO_POSITION: i32 = i32::MIN;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub start: u32,
    pub end: u32,
}

/// An image in a note: the offset of its placeholder and the file it lives in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteImage {
    pub offset: u32,
    pub file: String,
}

/// The colour of a note. The numbers are what notes store, so they never change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Style {
    Yellow = 1,
    Pink,
    Green,
    Purple,
    Blue,
    Gray,
    Charcoal,
    Window,
    Orange,
    Peach,
    Coral,
    Red,
    Rose,
    Lavender,
    Indigo,
    Cyan,
    Aqua,
    Mint,
    Lime,
    Sand,
}

impl Style {
    pub const ALL: [Style; 20] = [
        Style::Yellow, Style::Pink, Style::Green, Style::Purple, Style::Blue,
        Style::Gray, Style::Charcoal, Style::Window, Style::Orange, Style::Peach,
        Style::Coral, Style::Red, Style::Rose, Style::Lavender, Style::Indigo,
        Style::Cyan, Style::Aqua, Style::Mint, Style::Lime, Style::Sand,
    ];

    pub fn from_number(n: i64) -> Option<Style> {
        Style::ALL.iter().copied().find(|s| *s as i64 == n)
    }

    /// The name used for the CSS class `style-<name>`.
    pub fn name(self) -> &'static str {
        match self {
            Style::Yellow => "yellow", Style::Pink => "pink", Style::Green => "green",
            Style::Purple => "purple", Style::Blue => "blue", Style::Gray => "gray",
            Style::Charcoal => "charcoal", Style::Window => "window", Style::Orange => "orange",
            Style::Peach => "peach", Style::Coral => "coral", Style::Red => "red",
            Style::Rose => "rose", Style::Lavender => "lavender", Style::Indigo => "indigo",
            Style::Cyan => "cyan", Style::Aqua => "aqua", Style::Mint => "mint",
            Style::Lime => "lime", Style::Sand => "sand",
        }
    }
}

mod iso_millis {
    use super::*;

    pub fn serialize<S: Serializer>(d: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        // the same text JavaScript's Date.toISOString() wrote, so old and new files match
        s.serialize_str(&d.to_rfc3339_opts(SecondsFormat::Millis, true))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        DateTime::deserialize(d)
    }
}

fn v1() -> u8 {
    1
}

/// A note as it is saved on disk, one JSON file per note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    #[serde(default = "v1")]
    pub v: u8,
    pub uuid: String,
    pub content: String,
    /// Empty when the note follows its content instead of having a title.
    #[serde(default)]
    pub title: String,
    pub style: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<i32>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(with = "iso_millis")]
    pub modified: DateTime<Utc>,
    pub width: i32,
    pub height: i32,
    #[serde(default)]
    pub open: bool,
    /// Left out when empty, so notes without images keep their old format.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<NoteImage>,
    #[serde(default)]
    pub keep_above: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
}

impl Note {
    pub fn new(style: Style, font_size: i32) -> Note {
        Note {
            v: 1,
            uuid: uuid::Uuid::new_v4().to_string(),
            content: String::new(),
            title: String::new(),
            style: style as u8,
            font_size: Some(font_size),
            tags: vec![],
            modified: Utc::now(),
            width: 300,
            height: 300,
            open: false,
            images: vec![],
            keep_above: false,
            x: None,
            y: None,
        }
    }

    /// Notes saved before titles could be edited stored the derived title: drop
    /// those, they are meant to keep following the content.
    pub fn normalized(mut self, image_label: &str) -> Note {
        if self.title == derive_title(&self.content, image_label) {
            self.title.clear();
        }
        self
    }

    /// The title as shown: the note's own, or one derived from its content.
    pub fn display_title(&self, image_label: &str) -> String {
        if self.title.is_empty() {
            derive_title(&self.content, image_label)
        } else {
            self.title.clone()
        }
    }

    /// The uuid becomes a file name, so only plain ones are accepted.
    pub fn has_safe_uuid(&self) -> bool {
        is_safe_uuid(&self.uuid)
    }

    pub fn style(&self) -> Style {
        Style::from_number(self.style as i64).unwrap_or(Style::Yellow)
    }
}

pub fn is_safe_uuid(uuid: &str) -> bool {
    !uuid.is_empty() && uuid.len() <= 64 && uuid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The title a note falls back to when it hasn't been given one: the start of
/// its first line.
pub fn derive_title(content: &str, image_label: &str) -> String {
    if content.is_empty() {
        return String::new();
    }
    // an image has no text of its own to start a title with
    let text: String = content.chars().filter(|c| *c != IMAGE_PLACEHOLDER).collect();
    if text.trim().is_empty() && text.len() != content.len() {
        return image_label.to_string();
    }
    let first = text.split('\n').next().unwrap_or("");
    let mut title: String = first.chars().take(20).collect();
    if title.chars().count() != text.chars().count() {
        title.push('…');
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_follows_first_line() {
        assert_eq!(derive_title("", "Image"), "");
        assert_eq!(derive_title("short", "Image"), "short");
        assert_eq!(derive_title("first\nsecond", "Image"), "first…");
        assert_eq!(derive_title("0123456789012345678901234", "Image"), "01234567890123456789…");
        assert_eq!(derive_title("\u{FFFC}", "Image"), "Image");
    }

    #[test]
    fn reads_notes_written_by_the_js_app() {
        let json = r#"{"v":1,"uuid":"11111111-1111-1111-1111-111111111111","content":"ciao","title":"ciao","style":3,"tags":[{"name":"bold","start":0,"end":2}],"modified":"2026-10-13T08:00:00.123Z","width":300,"height":500,"open":true,"keep_above":false}"#;
        let note: Note = serde_json::from_str(json).unwrap();
        let note = note.normalized("Image");
        assert_eq!(note.title, "");
        assert_eq!(note.style(), Style::Green);
        assert_eq!(note.font_size, None);
        let out = serde_json::to_string(&note).unwrap();
        assert!(out.contains("\"modified\":\"2026-10-13T08:00:00.123Z\""));
        assert!(!out.contains("images") && !out.contains("\"x\""));
    }

    #[test]
    fn unsafe_uuids_are_refused() {
        assert!(is_safe_uuid("11111111-1111-1111-1111-111111111111"));
        assert!(!is_safe_uuid("../etc/passwd"));
        assert!(!is_safe_uuid(""));
    }
}

//! Everything about a note that does not need a window: the data, the files
//! it lives in, its images, and moving notes in and out of the app.

pub mod exchange;
pub mod images;
pub mod lists;
pub mod links;
pub mod migration;
pub mod note;
pub mod store;

pub use note::{derive_title, Note, NoteImage, Style, Tag, IMAGE_PLACEHOLDER, NO_POSITION};

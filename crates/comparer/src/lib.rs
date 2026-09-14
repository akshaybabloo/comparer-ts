use std::fmt;

use serde::Serialize;
use similar::{ChangeTag, TextDiff};
use ts_rs::TS;
use wasm_bindgen::prelude::*;

mod folder;
mod hasher;
mod images;

pub use folder::FolderComparer;
pub use hasher::Hasher;
pub use images::{ImageComparison, compare_images, compare_images_rgba};

/// An error from any comparison, thrown into JavaScript as an `Error` with this message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompareError(pub(crate) String);

impl fmt::Display for CompareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CompareError {}

impl From<CompareError> for JsValue {
    fn from(error: CompareError) -> JsValue {
        JsError::new(&error.0).into()
    }
}

/// Installs a panic hook so Rust panics surface as readable JS console errors
/// instead of an opaque `unreachable` trap.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
}

/// Strips the trailing line terminator from a line produced by `TextDiff::from_lines`.
///
/// Only a terminator is removed: a lone `\r` is only dropped when it directly
/// precedes the `\n` it belongs to, so a carriage return that is genuine content
/// (a `\r` at the end of a file with no final newline) is preserved.
fn strip_line_ending(value: &str) -> &str {
    match value.strip_suffix('\n') {
        Some(rest) => rest.strip_suffix('\r').unwrap_or(rest),
        None => value,
    }
}

fn tag_name(tag: ChangeTag) -> &'static str {
    match tag {
        ChangeTag::Delete => "delete",
        ChangeTag::Insert => "insert",
        ChangeTag::Equal => "equal",
    }
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct LineDiff {
    pub tag: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub value: String,
    pub missing_newline: bool,
}

/// Generates a line-by-line diff between two texts and returns it as a JSON string.
///
/// Each entry covers one whole line. For intra-line (word level) highlighting,
/// use [`generate_inline_diff`].
#[wasm_bindgen]
pub fn generate_diff(text1: &str, text2: &str) -> String {
    let diff = TextDiff::from_lines(text1, text2);

    let lines: Vec<LineDiff> = diff
        .iter_all_changes()
        .map(|change| LineDiff {
            tag: tag_name(change.tag()).to_string(),
            old_line: change.old_index(),
            new_line: change.new_index(),
            value: strip_line_ending(change.value()).to_string(),
            missing_newline: change.missing_newline(),
        })
        .collect();

    serde_json::to_string(&lines).expect("LineDiff is infallible to serialize")
}

#[derive(Serialize, TS, Clone)]
#[ts(export)]
pub struct Segment {
    pub emphasized: bool,
    pub value: String,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct InlineLineDiff {
    pub tag: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub segments: Vec<Segment>,
    /// See [`LineDiff::missing_newline`].
    pub missing_newline: bool,
}

/// Like [`generate_diff`], but each line is additionally split into segments
/// marking which parts of it actually changed, for word-level highlighting.
///
/// This runs a second diff pass within each changed line, so it costs more than
/// [`generate_diff`]; prefer the plain line diff when highlighting is not needed.
#[wasm_bindgen]
pub fn generate_inline_diff(text1: &str, text2: &str) -> String {
    let diff = TextDiff::from_lines(text1, text2);

    let lines: Vec<InlineLineDiff> = diff
        .iter_all_inline_changes()
        .map(|change| {
            // Stripping the line terminator empties the segment that carried it,
            // so drop empties rather than emitting blank spans to consumers.
            let segments = change
                .iter_strings_lossy()
                .filter_map(|(emphasized, value)| {
                    let value = strip_line_ending(&value);
                    (!value.is_empty()).then(|| Segment {
                        emphasized,
                        value: value.to_string(),
                    })
                })
                .collect();

            InlineLineDiff {
                tag: tag_name(change.tag()).to_string(),
                old_line: change.old_index(),
                new_line: change.new_index(),
                segments,
                missing_newline: change.missing_newline(),
            }
        })
        .collect();

    serde_json::to_string(&lines).expect("InlineLineDiff is infallible to serialize")
}

//! Line and word level text diffing.
//!
//! Two texts are compared line by line with [similar](https://github.com/mitsuhiko/similar).
//! [`diff_lines`] reports whole lines; [`diff_inline`] also splits each line into
//! segments marking which words actually changed.
//!
//! The line diff itself is one pass, but it comes out as a list of independent
//! operations: runs of equal, deleted, inserted or replaced lines. Expanding those into
//! rows — and, for the inline diff, running the second, word-level diff inside every
//! replaced run — is done per operation. Natively the operations run in parallel on
//! rayon; on WebAssembly, which has no threads, they run one after another. Either way
//! the rows come out in document order.

use similar::{ChangeTag, DiffOp, TextDiff};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "ts")]
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "lowercase")
)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum LineTag {
    Equal,
    Delete,
    Insert,
}

impl From<ChangeTag> for LineTag {
    fn from(tag: ChangeTag) -> LineTag {
        match tag {
            ChangeTag::Equal => LineTag::Equal,
            ChangeTag::Delete => LineTag::Delete,
            ChangeTag::Insert => LineTag::Insert,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct LineDiff {
    pub tag: LineTag,
    /// Zero-based line number in the old text, or null for an inserted line.
    pub old_line: Option<usize>,
    /// Zero-based line number in the new text, or null for a deleted line.
    pub new_line: Option<usize>,
    /// The line, without its `\n` or `\r\n`.
    pub value: String,
    /// The line had no trailing newline, which `value` alone cannot show.
    pub missing_newline: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct Segment {
    /// This part of the line changed.
    pub emphasized: bool,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct InlineLineDiff {
    pub tag: LineTag,
    /// See [`LineDiff::old_line`].
    pub old_line: Option<usize>,
    /// See [`LineDiff::new_line`].
    pub new_line: Option<usize>,
    /// The line split into changed and unchanged parts. Never empty strings, so a
    /// blank line has no segments at all.
    pub segments: Vec<Segment>,
    /// See [`LineDiff::missing_newline`].
    pub missing_newline: bool,
}

/// Compares two texts line by line.
///
/// Each row covers one whole line. For word-level highlighting, use [`diff_inline`].
pub fn diff_lines(old: &str, new: &str) -> Vec<LineDiff> {
    let diff = TextDiff::from_lines(old, new);
    expand_ops(diff.ops(), |op| {
        diff.iter_changes(op)
            .map(|change| LineDiff {
                tag: change.tag().into(),
                old_line: change.old_index(),
                new_line: change.new_index(),
                value: strip_line_ending(change.value()).to_string(),
                missing_newline: change.missing_newline(),
            })
            .collect()
    })
}

/// Like [`diff_lines`], but each line is also split into segments marking which parts
/// of it changed.
///
/// This runs a second diff inside every replaced run of lines, so it costs more than
/// [`diff_lines`]; prefer the plain line diff when highlighting is not needed.
pub fn diff_inline(old: &str, new: &str) -> Vec<InlineLineDiff> {
    let diff = TextDiff::from_lines(old, new);
    expand_ops(diff.ops(), |op| {
        diff.iter_inline_changes(op)
            .map(|change| {
                // Stripping the line terminator empties the segment that carried it,
                // so drop empties rather than handing out blank segments.
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
                    tag: change.tag().into(),
                    old_line: change.old_index(),
                    new_line: change.new_index(),
                    segments,
                    missing_newline: change.missing_newline(),
                }
            })
            .collect()
    })
}

/// Strips the trailing line terminator from a line produced by `TextDiff::from_lines`.
///
/// Only a terminator is removed: a lone `\r` is only dropped when it directly
/// precedes the `\n` it belongs to, so a carriage return that is genuine content
/// (a `\r` at the end of a text with no final newline) is preserved.
fn strip_line_ending(value: &str) -> &str {
    match value.strip_suffix('\n') {
        Some(rest) => rest.strip_suffix('\r').unwrap_or(rest),
        None => value,
    }
}

/// Expands every operation into its rows on rayon, keeping document order. Each one
/// only reads the finished line diff, so they share nothing to lock.
#[cfg(not(target_arch = "wasm32"))]
fn expand_ops<T: Send>(ops: &[DiffOp], expand: impl Fn(&DiffOp) -> Vec<T> + Send + Sync) -> Vec<T> {
    use rayon::prelude::*;

    ops.par_iter().flat_map_iter(expand).collect()
}

/// One after another: there is only the one thread to run them on.
#[cfg(target_arch = "wasm32")]
fn expand_ops<T>(ops: &[DiffOp], expand: impl Fn(&DiffOp) -> Vec<T>) -> Vec<T> {
    ops.iter().flat_map(expand).collect()
}

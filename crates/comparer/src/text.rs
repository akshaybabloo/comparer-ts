use wasm_bindgen::prelude::*;

/// Generates a line-by-line diff between two texts, as a JSON array of `LineDiff`.
///
/// Each entry covers one whole line. For word-level highlighting, use
/// [`generate_inline_diff`].
#[wasm_bindgen]
pub fn generate_diff(text1: &str, text2: &str) -> String {
    serde_json::to_string(&text_diff::diff_lines(text1, text2))
        .expect("LineDiff is infallible to serialize")
}

/// Like [`generate_diff`], but each line is also split into segments marking which
/// parts of it changed, as a JSON array of `InlineLineDiff`.
///
/// This runs a second diff inside every replaced run of lines, so it costs more than
/// [`generate_diff`]; prefer the plain line diff when highlighting is not needed.
#[wasm_bindgen]
pub fn generate_inline_diff(text1: &str, text2: &str) -> String {
    serde_json::to_string(&text_diff::diff_inline(text1, text2))
        .expect("InlineLineDiff is infallible to serialize")
}

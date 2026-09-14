//! The WebAssembly bindings behind comparer-ts.
//!
//! The comparisons themselves live in their own crates — `text-diff`, `folder-diff` and
//! `image-diff` — which also build natively. This crate only adapts them to JavaScript:
//! results cross as JSON or typed arrays, and errors become thrown `Error`s.

use std::fmt;

use wasm_bindgen::prelude::*;

mod folder;
mod hasher;
mod images;
mod text;

pub use folder::FolderComparer;
pub use hasher::Hasher;
pub use images::{ImageComparison, ImagePair, compare_images, compare_images_rgba};
pub use text::{generate_diff, generate_inline_diff};

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

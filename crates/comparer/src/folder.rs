use folder_diff::{DiffError, FsEntry, Side};
use wasm_bindgen::prelude::*;

use crate::CompareError;

impl From<DiffError> for CompareError {
    fn from(error: DiffError) -> CompareError {
        CompareError(error.to_string())
    }
}

/// Compares two folder listings for JavaScript.
///
/// WebAssembly has no filesystem, so the host lists both folders and hashes their
/// files, driving the comparison in three steps: construct it from both listings,
/// record a digest (see [`crate::Hasher`]) or an error for every job in
/// `pending_hashes`, then call `finish` for the tree. Listings and results cross as
/// JSON, in the shapes of `folder-diff`'s types.
#[wasm_bindgen]
pub struct FolderComparer(folder_diff::FolderComparer);

#[wasm_bindgen]
impl FolderComparer {
    /// Takes both listings as JSON arrays of `FsEntry`.
    #[wasm_bindgen(constructor)]
    pub fn new(left_json: &str, right_json: &str) -> Result<FolderComparer, CompareError> {
        let parse = |side: Side, json: &str| {
            serde_json::from_str::<Vec<FsEntry>>(json)
                .map_err(|error| CompareError(format!("{side} listing is invalid: {error}")))
        };
        let left = parse(Side::Left, left_json)?;
        let right = parse(Side::Right, right_json)?;
        Ok(FolderComparer(folder_diff::FolderComparer::new(
            left, right,
        )?))
    }

    /// The files to hash, as a JSON array of `HashJob`, sorted by path.
    pub fn pending_hashes(&self) -> String {
        serde_json::to_string(self.0.jobs()).expect("HashJob is infallible to serialize")
    }

    /// Records the digest of a job's content.
    pub fn set_hash(&mut self, id: u32, hash: &str) -> Result<(), CompareError> {
        Ok(self.0.set_hash(id, hash)?)
    }

    /// Records why a job's content could not be read, leaving that file `unknown`.
    pub fn set_error(&mut self, id: u32, message: &str) -> Result<(), CompareError> {
        Ok(self.0.set_error(id, message)?)
    }

    /// Returns the `FolderDiff` as JSON, consuming the comparer.
    pub fn finish(self) -> String {
        serde_json::to_string(&self.0.diff()).expect("FolderDiff is infallible to serialize")
    }
}

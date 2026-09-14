use wasm_bindgen::prelude::*;

/// Incremental XXH3-128 content hasher, for JavaScript to feed a file a chunk at a
/// time, so no file is ever copied into WebAssembly memory as a whole.
#[wasm_bindgen]
pub struct Hasher(folder_diff::Hasher);

impl Default for Hasher {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Hasher {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Hasher {
        Hasher(folder_diff::Hasher::new())
    }

    /// Adds the next chunk of content. Chunk boundaries do not affect the digest.
    pub fn update(&mut self, chunk: &[u8]) {
        self.0.update(chunk);
    }

    /// Returns the digest as 32 lowercase hex characters, consuming the hasher.
    pub fn finish(self) -> String {
        self.0.finish()
    }
}

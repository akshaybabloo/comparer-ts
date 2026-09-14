use xxhash_rust::xxh3::Xxh3;

/// Incremental XXH3-128 content hasher.
///
/// Fed a chunk at a time, so a file of any size is hashed without being held in
/// memory as a whole. The digest identifies content for change detection; it is not
/// a cryptographic hash.
pub struct Hasher {
    state: Box<Xxh3>,
}

impl Default for Hasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher {
    pub fn new() -> Hasher {
        Hasher {
            state: Box::new(Xxh3::new()),
        }
    }

    /// Adds the next chunk of content. Chunk boundaries do not affect the digest.
    pub fn update(&mut self, chunk: &[u8]) {
        self.state.update(chunk);
    }

    /// Returns the digest as 32 lowercase hex characters, consuming the hasher.
    pub fn finish(self) -> String {
        format!("{:032x}", self.state.digest128())
    }
}

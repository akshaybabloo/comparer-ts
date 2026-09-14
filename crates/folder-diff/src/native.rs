//! Reading folders from disk, natively only: WebAssembly has no filesystem, so there
//! the host lists the folders and hashes the files instead.

use std::fs::{self, File, FileType, Metadata};
use std::io::{self, Read};
use std::path::Path;

use rayon::prelude::*;

use crate::{DiffError, EntryKind, FolderComparer, FolderDiff, FsEntry, Hasher, Side};

/// Files up to this size are hashed from a single read; larger ones stream in chunks
/// of this size, so memory stays bounded however large a file is.
const HASH_CHUNK_BYTES: u64 = 1024 * 1024;

/// Compares two folders on disk, by content and metadata.
///
/// Both folders are listed at once, then every file whose size matches on both sides
/// is hashed, spread across the current rayon pool. A file or folder that cannot be
/// read shows up in the tree as `unknown` rather than failing the comparison; only a
/// folder that cannot be listed at all does.
pub fn diff_folders(left: &Path, right: &Path) -> Result<FolderDiff, DiffError> {
    let (left_entries, right_entries) = rayon::join(|| list_folder(left), || list_folder(right));
    let unreadable = |side, error: io::Error| DiffError::Unreadable {
        side,
        reason: error.to_string(),
    };
    let left_entries = left_entries.map_err(|error| unreadable(Side::Left, error))?;
    let right_entries = right_entries.map_err(|error| unreadable(Side::Right, error))?;

    let mut comparer = FolderComparer::new(left_entries, right_entries)?;
    let outcomes: Vec<(u32, io::Result<String>)> = comparer
        .jobs()
        .par_iter()
        .map(|job| {
            let root = match job.side {
                Side::Left => left,
                Side::Right => right,
            };
            (job.id, hash_file(&root.join(&job.path), job.size))
        })
        .collect();

    for (id, outcome) in outcomes {
        match outcome {
            Ok(hash) => comparer.set_hash(id, hash),
            Err(error) => comparer.set_error(id, error.to_string()),
        }
        .expect("job ids come from the comparer itself");
    }
    Ok(comparer.diff())
}

/// Lists every entry under `root` — hidden files, `.git`, `node_modules` and special
/// files included — as the flat listing [`FolderComparer`] compares.
///
/// Folders are read in parallel on rayon. Symlinks are recorded with their target but
/// never followed, which keeps a link cycle from looping and a linked folder from being
/// listed twice. An entry that cannot be stat'ed, or a folder that cannot be listed, is
/// still reported, carrying an `error`, rather than silently dropped. Only a `root` that
/// cannot be listed fails the call.
///
/// Names that are not valid UTF-8 are converted lossily, so such an entry can only be
/// compared by its metadata.
pub fn list_folder(root: &Path) -> io::Result<Vec<FsEntry>> {
    read_folder(root, "")
}

fn read_folder(folder: &Path, relative: &str) -> io::Result<Vec<FsEntry>> {
    let dirents = fs::read_dir(folder)?.collect::<io::Result<Vec<_>>>()?;

    Ok(dirents
        .into_par_iter()
        .flat_map_iter(|dirent| {
            let name = dirent.file_name().to_string_lossy().into_owned();
            let path = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            let absolute = dirent.path();
            let mut entry = describe(&absolute, path, dirent.file_type().ok());

            let mut inside = Vec::new();
            if entry.kind == EntryKind::Dir && entry.error.is_none() {
                match read_folder(&absolute, &entry.path) {
                    Ok(found) => inside = found,
                    Err(error) => entry.error = Some(error.to_string()),
                }
            }
            std::iter::once(entry).chain(inside)
        })
        .collect())
}

fn describe(absolute: &Path, path: String, listed_type: Option<FileType>) -> FsEntry {
    let metadata = match fs::symlink_metadata(absolute) {
        Ok(metadata) => metadata,
        Err(error) => {
            // Keep whatever the directory listing already said about the entry, so it
            // still shows up in the tree as the right kind.
            return FsEntry {
                path,
                kind: listed_type.as_ref().map_or(EntryKind::File, kind_of),
                size: 0,
                mode: 0,
                uid: 0,
                gid: 0,
                mtime_ns: "0".to_string(),
                link_target: None,
                error: Some(error.to_string()),
            };
        }
    };

    let kind = kind_of(&metadata.file_type());
    let (mode, uid, gid) = ownership(&metadata);
    let mut entry = FsEntry {
        path,
        kind,
        size: metadata.len(),
        mode,
        uid,
        gid,
        mtime_ns: mtime_ns(&metadata).to_string(),
        link_target: None,
        error: None,
    };

    if kind == EntryKind::Symlink {
        match fs::read_link(absolute) {
            Ok(target) => entry.link_target = Some(target.to_string_lossy().into_owned()),
            Err(error) => entry.error = Some(error.to_string()),
        }
    }
    entry
}

fn kind_of(file_type: &FileType) -> EntryKind {
    if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Dir
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

#[cfg(unix)]
fn ownership(metadata: &Metadata) -> (u32, u32, u32) {
    use std::os::unix::fs::MetadataExt;

    (metadata.mode(), metadata.uid(), metadata.gid())
}

/// A POSIX-style mode where there is none, built the way Node's `lstat` does on
/// Windows, so listings from either source compare alike: the type bits, plus
/// read-only or read-write for everyone. There is no owner to report.
#[cfg(not(unix))]
fn ownership(metadata: &Metadata) -> (u32, u32, u32) {
    let file_type = metadata.file_type();
    let type_bits = if file_type.is_symlink() {
        0o120_000
    } else if file_type.is_dir() {
        0o040_000
    } else {
        0o100_000
    };
    let access = if metadata.permissions().readonly() {
        0o444
    } else {
        0o666
    };
    let execute = if file_type.is_dir() { 0o111 } else { 0 };
    (type_bits | access | execute, 0, 0)
}

#[cfg(unix)]
fn mtime_ns(metadata: &Metadata) -> i128 {
    use std::os::unix::fs::MetadataExt;

    i128::from(metadata.mtime()) * 1_000_000_000 + i128::from(metadata.mtime_nsec())
}

#[cfg(not(unix))]
fn mtime_ns(metadata: &Metadata) -> i128 {
    use std::time::UNIX_EPOCH;

    match metadata.modified() {
        Ok(time) => match time.duration_since(UNIX_EPOCH) {
            Ok(after) => after.as_nanos() as i128,
            Err(before) => -(before.duration().as_nanos() as i128),
        },
        Err(_) => 0,
    }
}

fn hash_file(path: &Path, size: u64) -> io::Result<String> {
    let mut hasher = Hasher::new();
    if size <= HASH_CHUNK_BYTES {
        // Most files are small, and one read beats setting up a buffered loop.
        hasher.update(&fs::read(path)?);
        return Ok(hasher.finish());
    }

    let mut file = File::open(path)?;
    let mut buffer = vec![0; HASH_CHUNK_BYTES as usize];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return Ok(hasher.finish()),
            Ok(read) => hasher.update(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

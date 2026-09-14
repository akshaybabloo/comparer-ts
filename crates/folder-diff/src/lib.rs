//! Folder comparison by content and metadata.
//!
//! Two folder listings are merged into one tree, and every entry gets a status: added,
//! deleted, modified, unchanged, or unknown when one side could not be read. An entry
//! present on both sides is modified if anything `lstat` reports differs — its kind,
//! size, permissions, owner, modification time or symlink target — or if its content
//! does, which is settled by hashing only the files whose sizes match.
//!
//! Natively, [`diff_folders`] does everything from two paths: both folders are listed
//! and their files hashed in parallel on rayon. On WebAssembly, which has no filesystem
//! and no threads, the host lists the folders and hashes the files itself, driving a
//! [`FolderComparer`] step by step.

use std::fmt;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "ts")]
use ts_rs::TS;

mod comparer;
mod hasher;
#[cfg(not(target_arch = "wasm32"))]
mod native;

pub use comparer::FolderComparer;
pub use hasher::Hasher;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{diff_folders, list_folder};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "lowercase")
)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    /// A FIFO, socket or device. Its metadata is compared, its content never read.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "lowercase")
)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum Side {
    Left,
    Right,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Side::Left => "left",
            Side::Right => "right",
        })
    }
}

/// One entry of a folder listing, as reported by `lstat`.
///
/// Every entry below the compared folder must be listed, folders included: an
/// entry whose parent folder is missing from the listing is rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct FsEntry {
    /// Relative to the compared folder, `/`-separated, with no `.` or `..` segments.
    /// Paths are case-sensitive, so `a.txt` and `A.txt` are different entries.
    pub path: String,
    pub kind: EntryKind,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub size: u64,
    /// The full POSIX `st_mode`, type bits included.
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    /// Modification time in nanoseconds since the epoch, as a decimal string so
    /// it keeps full precision in JSON.
    pub mtime_ns: String,
    /// Where a symlink points. Set for symlinks only.
    #[cfg_attr(feature = "serde", serde(default))]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub link_target: Option<String>,
    /// Why this entry could not be fully read. Its metadata is then not trusted.
    #[cfg_attr(feature = "serde", serde(default))]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "lowercase")
)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum ChangeStatus {
    Added,
    Deleted,
    Modified,
    Unchanged,
    /// Present on both sides, but one of them could not be read.
    Unknown,
}

/// Why an entry present on both sides is [`ChangeStatus::Modified`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "snake_case")
)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum ChangeReason {
    /// A different kind of entry, such as a file replaced by a folder.
    Kind,
    Content,
    Size,
    Permissions,
    /// A different owning user or group.
    Owner,
    ModifiedTime,
    LinkTarget,
}

/// A file whose content has to be hashed before the comparison can finish.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct HashJob {
    pub id: u32,
    pub side: Side,
    pub path: String,
    /// The file's size in the listing, the same on both sides, so a reader can pick
    /// between reading it whole and streaming it.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    /// The entry's kind on the left, or null when it only exists on the right.
    pub left_kind: Option<EntryKind>,
    /// The entry's kind on the right, or null when it only exists on the left.
    pub right_kind: Option<EntryKind>,
    pub status: ChangeStatus,
    /// Every way the entry differs; empty unless `status` is `modified`.
    pub reasons: Vec<ChangeReason>,
    /// This entry or anything inside it is not `unchanged`.
    pub has_changes: bool,
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub error: Option<String>,
    /// Folders first, then by name ignoring case.
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    pub fn is_folder(&self) -> bool {
        self.left_kind == Some(EntryKind::Dir) || self.right_kind == Some(EntryKind::Dir)
    }
}

/// Entries counted by status, at every depth.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct FolderStats {
    pub added: usize,
    pub deleted: usize,
    pub modified: usize,
    pub unchanged: usize,
    pub unknown: usize,
}

impl FolderStats {
    fn count(&mut self, status: ChangeStatus) {
        match status {
            ChangeStatus::Added => self.added += 1,
            ChangeStatus::Deleted => self.deleted += 1,
            ChangeStatus::Modified => self.modified += 1,
            ChangeStatus::Unchanged => self.unchanged += 1,
            ChangeStatus::Unknown => self.unknown += 1,
        }
    }

    fn add(&mut self, other: FolderStats) {
        self.added += other.added;
        self.deleted += other.deleted;
        self.modified += other.modified;
        self.unchanged += other.unchanged;
        self.unknown += other.unknown;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct FolderDiff {
    /// The top-level entries of both folders.
    pub entries: Vec<TreeNode>,
    pub stats: FolderStats,
    /// Every entry is unchanged.
    pub identical: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffError {
    /// A listing entry is malformed, listed twice, or inside something that is not a
    /// listed folder.
    InvalidEntry {
        side: Side,
        path: String,
        reason: &'static str,
    },
    /// A hash or error was recorded for a job the comparer never handed out.
    UnknownJob(u32),
    /// A compared folder itself could not be listed.
    Unreadable { side: Side, reason: String },
}

impl fmt::Display for DiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiffError::InvalidEntry { side, path, reason } => {
                write!(f, "{side} entry \"{path}\": {reason}")
            }
            DiffError::UnknownJob(id) => write!(f, "unknown hash job {id}"),
            DiffError::Unreadable { side, reason } => {
                write!(f, "{side} folder could not be listed: {reason}")
            }
        }
    }
}

impl std::error::Error for DiffError {}

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wasm_bindgen::prelude::*;

/// File type bits of a POSIX `st_mode`.
const S_IFMT: u32 = 0o170_000;
/// Permission bits of a POSIX `st_mode`, including setuid, setgid and sticky.
const PERMISSION_BITS: u32 = 0o7_777;

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    /// A FIFO, socket or device. Its metadata is compared, its content never read.
    Other,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
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
#[derive(Deserialize, TS, Clone, Debug)]
#[ts(export)]
pub struct FsEntry {
    /// Relative to the compared folder, `/`-separated, with no `.` or `..` segments.
    /// Paths are case-sensitive, so `a.txt` and `A.txt` are different entries.
    pub path: String,
    pub kind: EntryKind,
    #[ts(type = "number")]
    pub size: u64,
    /// The full POSIX `st_mode`, type bits included.
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    /// Modification time in nanoseconds since the epoch, as a decimal string so
    /// it keeps full precision in JSON.
    pub mtime_ns: String,
    /// Where a symlink points. Set for symlinks only.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub link_target: Option<String>,
    /// Why this entry could not be fully read. Its metadata is then not trusted.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub error: Option<String>,
}

#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ChangeStatus {
    Added,
    Deleted,
    Modified,
    Unchanged,
    /// Present on both sides, but one of them could not be read.
    Unknown,
}

/// Why an entry present on both sides is [`ChangeStatus::Modified`].
#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
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
#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[ts(export)]
pub struct HashJob {
    pub id: u32,
    pub side: Side,
    pub path: String,
}

#[derive(Serialize, TS, Clone, Debug)]
#[ts(export)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
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
#[derive(Serialize, TS, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[ts(export)]
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
}

#[derive(Serialize, TS, Clone, Debug)]
#[ts(export)]
pub struct FolderDiff {
    /// The top-level entries of both folders.
    pub entries: Vec<TreeNode>,
    pub stats: FolderStats,
    /// Every entry is unchanged.
    pub identical: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompareError(pub(crate) String);

impl CompareError {
    fn entry(side: Side, path: &str, reason: &str) -> Self {
        CompareError(format!("{side} entry \"{path}\": {reason}"))
    }
}

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

/// One side's listing, indexed for the walk.
struct Tree {
    entries: BTreeMap<String, FsEntry>,
    /// Names directly inside each folder, keyed by folder path (`""` is the root).
    children: HashMap<String, Vec<String>>,
}

impl Tree {
    fn new(side: Side, listing: Vec<FsEntry>) -> Result<Tree, CompareError> {
        let mut entries = BTreeMap::new();
        let mut children: HashMap<String, Vec<String>> = HashMap::new();

        for entry in listing {
            let path = entry.path.as_str();
            validate_path(path).map_err(|reason| CompareError::entry(side, path, reason))?;
            if entry.error.is_none() && entry.mtime_ns.parse::<i128>().is_err() {
                return Err(CompareError::entry(
                    side,
                    path,
                    "mtime_ns must be an integer",
                ));
            }
            if entries.contains_key(path) {
                return Err(CompareError::entry(side, path, "listed more than once"));
            }

            let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
            children
                .entry(parent.to_string())
                .or_default()
                .push(name.to_string());
            entries.insert(entry.path.clone(), entry);
        }

        for parent in children.keys().filter(|parent| !parent.is_empty()) {
            match entries.get(parent) {
                Some(entry) if entry.kind == EntryKind::Dir => {}
                Some(_) => {
                    return Err(CompareError::entry(
                        side,
                        parent,
                        "has entries inside it but is not a folder",
                    ));
                }
                None => {
                    return Err(CompareError::entry(
                        side,
                        parent,
                        "has entries inside it but is not listed",
                    ));
                }
            }
        }

        Ok(Tree { entries, children })
    }
}

fn validate_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("path is empty");
    }
    for segment in path.split('/') {
        match segment {
            "" => return Err("path must be relative, with no empty segments"),
            "." | ".." => return Err("path must not contain . or .. segments"),
            _ => {}
        }
    }
    Ok(())
}

/// Joins per-side problems into one message, or `None` when there are none.
fn sided_error(left: Option<&str>, right: Option<&str>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(format!("left: {left}; right: {right}")),
        (Some(left), None) => Some(format!("left: {left}")),
        (None, Some(right)) => Some(format!("right: {right}")),
        (None, None) => None,
    }
}

type HashResult = Option<Result<String, String>>;

fn hash_problem(result: &HashResult) -> Option<&str> {
    match result {
        None => Some("content was not hashed"),
        Some(Err(message)) => Some(message),
        Some(Ok(_)) => None,
    }
}

/// Compares two folder listings.
///
/// Used in three steps: construct it from both listings, feed it a digest (see
/// [`crate::Hasher`]) or an error for every job in `pending_hashes`, then call
/// `finish` for the tree. Only files present on both sides with equal sizes
/// need hashing; any other content difference is already known.
#[wasm_bindgen]
pub struct FolderComparer {
    left: Tree,
    right: Tree,
    jobs: Vec<HashJob>,
    /// Outcome of each job, indexed by job id.
    hashes: Vec<HashResult>,
    /// The left side's job id for each path being hashed; the right side's is the next id.
    job_ids: HashMap<String, u32>,
}

impl FolderComparer {
    pub fn from_entries(left: Vec<FsEntry>, right: Vec<FsEntry>) -> Result<Self, CompareError> {
        let left = Tree::new(Side::Left, left)?;
        let right = Tree::new(Side::Right, right)?;

        let mut jobs = Vec::new();
        let mut job_ids = HashMap::new();
        for (path, l) in &left.entries {
            let Some(r) = right.entries.get(path) else {
                continue;
            };
            let comparable = l.error.is_none() && r.error.is_none();
            if comparable
                && l.kind == EntryKind::File
                && r.kind == EntryKind::File
                && l.size == r.size
            {
                let id = jobs.len() as u32;
                job_ids.insert(path.clone(), id);
                jobs.push(HashJob {
                    id,
                    side: Side::Left,
                    path: path.clone(),
                });
                jobs.push(HashJob {
                    id: id + 1,
                    side: Side::Right,
                    path: path.clone(),
                });
            }
        }

        Ok(FolderComparer {
            left,
            right,
            hashes: vec![None; jobs.len()],
            jobs,
            job_ids,
        })
    }

    pub fn jobs(&self) -> &[HashJob] {
        &self.jobs
    }

    fn record(&mut self, id: u32, result: Result<String, String>) -> Result<(), CompareError> {
        let slot = self
            .hashes
            .get_mut(id as usize)
            .ok_or_else(|| CompareError(format!("unknown hash job {id}")))?;
        *slot = Some(result);
        Ok(())
    }

    /// Builds the tree. Jobs never given a hash or error leave their file `unknown`.
    pub fn diff(self) -> FolderDiff {
        let mut stats = FolderStats::default();
        let entries = self.children("", &mut stats);
        FolderDiff {
            identical: entries.iter().all(|node| !node.has_changes),
            entries,
            stats,
        }
    }

    fn children(&self, path: &str, stats: &mut FolderStats) -> Vec<TreeNode> {
        // Only folders have children, so a path that is a file on one side
        // contributes names from the other side alone.
        let names: BTreeSet<&str> = [&self.left, &self.right]
            .into_iter()
            .filter_map(|tree| tree.children.get(path))
            .flatten()
            .map(String::as_str)
            .collect();

        let mut nodes: Vec<TreeNode> = names
            .into_iter()
            .map(|name| {
                let child = if path.is_empty() {
                    name.to_string()
                } else {
                    format!("{path}/{name}")
                };
                self.node(name, child, stats)
            })
            .collect();

        nodes.sort_by_cached_key(|node| {
            (
                !node.is_folder(),
                node.name.to_lowercase(),
                node.name.clone(),
            )
        });
        nodes
    }

    fn node(&self, name: &str, path: String, stats: &mut FolderStats) -> TreeNode {
        let left = self.left.entries.get(&path);
        let right = self.right.entries.get(&path);

        let (status, reasons, error) = match (left, right) {
            (Some(l), None) => (ChangeStatus::Deleted, Vec::new(), l.error.clone()),
            (None, Some(r)) => (ChangeStatus::Added, Vec::new(), r.error.clone()),
            (Some(l), Some(r)) => self.compare(&path, l, r),
            (None, None) => unreachable!("child names only come from listed entries"),
        };
        stats.count(status);

        let children = self.children(&path, stats);
        let has_changes =
            status != ChangeStatus::Unchanged || children.iter().any(|child| child.has_changes);

        TreeNode {
            name: name.to_string(),
            path,
            left_kind: left.map(|entry| entry.kind),
            right_kind: right.map(|entry| entry.kind),
            status,
            reasons,
            has_changes,
            error,
            children,
        }
    }

    fn compare(
        &self,
        path: &str,
        l: &FsEntry,
        r: &FsEntry,
    ) -> (ChangeStatus, Vec<ChangeReason>, Option<String>) {
        if l.error.is_some() || r.error.is_some() {
            let error = sided_error(l.error.as_deref(), r.error.as_deref());
            return (ChangeStatus::Unknown, Vec::new(), error);
        }

        let mut reasons = Vec::new();
        let mut error = None;

        // Two "other" entries can still be different kinds, such as a FIFO and
        // a socket, which only the type bits tell apart.
        if l.kind != r.kind || l.mode & S_IFMT != r.mode & S_IFMT {
            reasons.push(ChangeReason::Kind);
        } else {
            if l.kind == EntryKind::File {
                match self.content_differs(path, l, r) {
                    Ok(true) => reasons.push(ChangeReason::Content),
                    Ok(false) => {}
                    Err(message) => error = Some(message),
                }
            }
            // A folder's size is filesystem bookkeeping rather than anything
            // stored in it, and a symlink's is the length of its target.
            if matches!(l.kind, EntryKind::File | EntryKind::Other) && l.size != r.size {
                reasons.push(ChangeReason::Size);
            }
            if l.mode & PERMISSION_BITS != r.mode & PERMISSION_BITS {
                reasons.push(ChangeReason::Permissions);
            }
            if l.uid != r.uid || l.gid != r.gid {
                reasons.push(ChangeReason::Owner);
            }
            if l.mtime_ns.parse::<i128>().ok() != r.mtime_ns.parse::<i128>().ok() {
                reasons.push(ChangeReason::ModifiedTime);
            }
            if l.link_target != r.link_target {
                reasons.push(ChangeReason::LinkTarget);
            }
        }

        // A known difference settles it even when the content could not be read.
        let status = if !reasons.is_empty() {
            ChangeStatus::Modified
        } else if error.is_some() {
            ChangeStatus::Unknown
        } else {
            ChangeStatus::Unchanged
        };
        (status, reasons, error)
    }

    fn content_differs(&self, path: &str, l: &FsEntry, r: &FsEntry) -> Result<bool, String> {
        if l.size != r.size {
            return Ok(true);
        }
        let Some(&id) = self.job_ids.get(path) else {
            return Err("content was not hashed".to_string());
        };
        let left = &self.hashes[id as usize];
        let right = &self.hashes[id as usize + 1];
        match (left, right) {
            (Some(Ok(left)), Some(Ok(right))) => Ok(left != right),
            _ => Err(sided_error(hash_problem(left), hash_problem(right)).unwrap_or_default()),
        }
    }
}

#[wasm_bindgen]
impl FolderComparer {
    /// Takes both listings as JSON arrays of [`FsEntry`].
    #[wasm_bindgen(constructor)]
    pub fn new(left_json: &str, right_json: &str) -> Result<FolderComparer, CompareError> {
        let parse = |side: Side, json: &str| {
            serde_json::from_str::<Vec<FsEntry>>(json)
                .map_err(|error| CompareError(format!("{side} listing is invalid: {error}")))
        };
        Self::from_entries(
            parse(Side::Left, left_json)?,
            parse(Side::Right, right_json)?,
        )
    }

    /// The files to hash, as a JSON array of [`HashJob`], sorted by path.
    pub fn pending_hashes(&self) -> String {
        serde_json::to_string(&self.jobs).expect("HashJob is infallible to serialize")
    }

    /// Records the digest of a job's content.
    pub fn set_hash(&mut self, id: u32, hash: &str) -> Result<(), CompareError> {
        self.record(id, Ok(hash.to_string()))
    }

    /// Records why a job's content could not be read, leaving that file `unknown`.
    pub fn set_error(&mut self, id: u32, message: &str) -> Result<(), CompareError> {
        self.record(id, Err(message.to_string()))
    }

    /// Returns the [`FolderDiff`] as JSON, consuming the comparer.
    pub fn finish(self) -> String {
        serde_json::to_string(&self.diff()).expect("FolderDiff is infallible to serialize")
    }
}

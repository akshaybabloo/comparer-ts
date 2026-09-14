use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::{
    ChangeReason, ChangeStatus, DiffError, EntryKind, FolderDiff, FolderStats, FsEntry, HashJob,
    Side, TreeNode,
};

/// File type bits of a POSIX `st_mode`.
const S_IFMT: u32 = 0o170_000;
/// Permission bits of a POSIX `st_mode`, including setuid, setgid and sticky.
const PERMISSION_BITS: u32 = 0o7_777;

/// One side's listing, indexed for the walk.
struct Tree {
    entries: BTreeMap<String, FsEntry>,
    /// Names directly inside each folder, keyed by folder path (`""` is the root).
    children: HashMap<String, Vec<String>>,
}

impl Tree {
    fn new(side: Side, listing: Vec<FsEntry>) -> Result<Tree, DiffError> {
        let invalid = |path: &str, reason| DiffError::InvalidEntry {
            side,
            path: path.to_string(),
            reason,
        };
        let mut entries = BTreeMap::new();
        let mut children: HashMap<String, Vec<String>> = HashMap::new();

        for entry in listing {
            let path = entry.path.as_str();
            validate_path(path).map_err(|reason| invalid(path, reason))?;
            if entry.error.is_none() && entry.mtime_ns.parse::<i128>().is_err() {
                return Err(invalid(path, "mtime_ns must be an integer"));
            }
            if entries.contains_key(path) {
                return Err(invalid(path, "listed more than once"));
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
                    return Err(invalid(parent, "has entries inside it but is not a folder"));
                }
                None => return Err(invalid(parent, "has entries inside it but is not listed")),
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
/// Used in three steps: construct it from both listings, record a digest (see
/// [`crate::Hasher`]) or an error for every job in [`FolderComparer::jobs`], then call
/// [`FolderComparer::diff`] for the tree. Only files present on both sides with equal
/// sizes need hashing; any other content difference is already known.
///
/// [`crate::diff_folders`] runs all three natively, straight from two paths.
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
    pub fn new(left: Vec<FsEntry>, right: Vec<FsEntry>) -> Result<FolderComparer, DiffError> {
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
                for (id, side) in [(id, Side::Left), (id + 1, Side::Right)] {
                    jobs.push(HashJob {
                        id,
                        side,
                        path: path.clone(),
                        size: l.size,
                    });
                }
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

    /// The files to hash, sorted by path, left before right.
    pub fn jobs(&self) -> &[HashJob] {
        &self.jobs
    }

    /// Records the digest of a job's content.
    pub fn set_hash(&mut self, id: u32, hash: impl Into<String>) -> Result<(), DiffError> {
        self.record(id, Ok(hash.into()))
    }

    /// Records why a job's content could not be read, leaving that file `unknown`.
    pub fn set_error(&mut self, id: u32, message: impl Into<String>) -> Result<(), DiffError> {
        self.record(id, Err(message.into()))
    }

    fn record(&mut self, id: u32, result: Result<String, String>) -> Result<(), DiffError> {
        let slot = self
            .hashes
            .get_mut(id as usize)
            .ok_or(DiffError::UnknownJob(id))?;
        *slot = Some(result);
        Ok(())
    }

    /// Builds the tree. Jobs never given a hash or error leave their file `unknown`.
    pub fn diff(self) -> FolderDiff {
        let (entries, stats) = self.children("");
        FolderDiff {
            identical: entries.iter().all(|node| !node.has_changes),
            entries,
            stats,
        }
    }

    fn children(&self, path: &str) -> (Vec<TreeNode>, FolderStats) {
        // Only folders have children, so a path that is a file on one side
        // contributes names from the other side alone.
        let names: BTreeSet<&str> = [&self.left, &self.right]
            .into_iter()
            .filter_map(|tree| tree.children.get(path))
            .flatten()
            .map(String::as_str)
            .collect();

        let children: Vec<(&str, String)> = names
            .into_iter()
            .map(|name| {
                let child = if path.is_empty() {
                    name.to_string()
                } else {
                    format!("{path}/{name}")
                };
                (name, child)
            })
            .collect();

        let mut stats = FolderStats::default();
        let mut nodes: Vec<TreeNode> = self
            .build_nodes(children)
            .into_iter()
            .map(|(node, node_stats)| {
                stats.add(node_stats);
                node
            })
            .collect();

        nodes.sort_by_cached_key(|node| {
            (
                !node.is_folder(),
                node.name.to_lowercase(),
                node.name.clone(),
            )
        });
        (nodes, stats)
    }

    /// Builds sibling subtrees in parallel on rayon: each only reads the listings and
    /// returns its own counts, so they share nothing to lock.
    #[cfg(not(target_arch = "wasm32"))]
    fn build_nodes(&self, children: Vec<(&str, String)>) -> Vec<(TreeNode, FolderStats)> {
        use rayon::prelude::*;

        children
            .into_par_iter()
            .map(|(name, path)| self.node(name, path))
            .collect()
    }

    /// One after another: there is only the one thread to run them on.
    #[cfg(target_arch = "wasm32")]
    fn build_nodes(&self, children: Vec<(&str, String)>) -> Vec<(TreeNode, FolderStats)> {
        children
            .into_iter()
            .map(|(name, path)| self.node(name, path))
            .collect()
    }

    fn node(&self, name: &str, path: String) -> (TreeNode, FolderStats) {
        let left = self.left.entries.get(&path);
        let right = self.right.entries.get(&path);

        let (status, reasons, error) = match (left, right) {
            (Some(l), None) => (ChangeStatus::Deleted, Vec::new(), l.error.clone()),
            (None, Some(r)) => (ChangeStatus::Added, Vec::new(), r.error.clone()),
            (Some(l), Some(r)) => self.compare(&path, l, r),
            (None, None) => unreachable!("child names only come from listed entries"),
        };

        let (children, mut stats) = self.children(&path);
        stats.count(status);
        let has_changes =
            status != ChangeStatus::Unchanged || children.iter().any(|child| child.has_changes);

        let node = TreeNode {
            name: name.to_string(),
            path,
            left_kind: left.map(|entry| entry.kind),
            right_kind: right.map(|entry| entry.kind),
            status,
            reasons,
            has_changes,
            error,
            children,
        };
        (node, stats)
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

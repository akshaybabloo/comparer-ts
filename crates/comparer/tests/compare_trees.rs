use comparer::{
    ChangeReason, ChangeStatus, EntryKind, FolderComparer, FolderDiff, FsEntry, Hasher, Side,
    TreeNode,
};

const FILE_MODE: u32 = 0o100_644;
const DIR_MODE: u32 = 0o040_755;
const LINK_MODE: u32 = 0o120_777;

fn file(path: &str, size: u64) -> FsEntry {
    FsEntry {
        path: path.to_string(),
        kind: EntryKind::File,
        size,
        mode: FILE_MODE,
        uid: 1000,
        gid: 1000,
        mtime_ns: "1700000000000000000".to_string(),
        link_target: None,
        error: None,
    }
}

fn dir(path: &str) -> FsEntry {
    FsEntry {
        kind: EntryKind::Dir,
        size: 4096,
        mode: DIR_MODE,
        ..file(path, 0)
    }
}

fn symlink(path: &str, target: &str) -> FsEntry {
    FsEntry {
        kind: EntryKind::Symlink,
        size: target.len() as u64,
        mode: LINK_MODE,
        link_target: Some(target.to_string()),
        ..file(path, 0)
    }
}

fn hash(content: &[u8]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(content);
    hasher.finish()
}

/// Runs a comparison, answering every hash job from the given contents.
fn compare_with(
    left: Vec<FsEntry>,
    right: Vec<FsEntry>,
    content: impl Fn(Side, &str) -> Option<&'static [u8]>,
) -> FolderDiff {
    let mut comparer = FolderComparer::from_entries(left, right).unwrap();
    for job in comparer.jobs().to_vec() {
        match content(job.side, &job.path) {
            Some(bytes) => comparer.set_hash(job.id, &hash(bytes)).unwrap(),
            None => comparer.set_error(job.id, "permission denied").unwrap(),
        }
    }
    comparer.diff()
}

fn compare(left: Vec<FsEntry>, right: Vec<FsEntry>) -> FolderDiff {
    compare_with(left, right, |_, _| Some(b"same"))
}

fn find<'a>(nodes: &'a [TreeNode], path: &str) -> &'a TreeNode {
    fn search<'a>(nodes: &'a [TreeNode], path: &str) -> Option<&'a TreeNode> {
        nodes.iter().find_map(|node| {
            if node.path == path {
                Some(node)
            } else {
                search(&node.children, path)
            }
        })
    }
    search(nodes, path).unwrap_or_else(|| panic!("no node at {path}"))
}

#[test]
fn identical_folders_are_unchanged() {
    let listing = || vec![dir("src"), file("src/a.txt", 4), file("b.txt", 4)];
    let diff = compare(listing(), listing());

    assert!(diff.identical);
    assert_eq!(diff.stats.unchanged, 3);
    assert!(diff.entries.iter().all(|node| !node.has_changes));
}

#[test]
fn files_only_on_one_side_are_added_or_deleted() {
    let diff = compare(vec![file("old.txt", 1)], vec![file("new.txt", 1)]);

    let old = find(&diff.entries, "old.txt");
    assert_eq!(old.status, ChangeStatus::Deleted);
    assert_eq!(
        (old.left_kind, old.right_kind),
        (Some(EntryKind::File), None)
    );

    let new = find(&diff.entries, "new.txt");
    assert_eq!(new.status, ChangeStatus::Added);
    assert_eq!(
        (new.left_kind, new.right_kind),
        (None, Some(EntryKind::File))
    );

    assert!(!diff.identical);
    assert_eq!((diff.stats.added, diff.stats.deleted), (1, 1));
}

#[test]
fn everything_inside_an_added_folder_is_added() {
    let diff = compare(
        vec![],
        vec![dir("lib"), dir("lib/deep"), file("lib/deep/x.rs", 3)],
    );

    for path in ["lib", "lib/deep", "lib/deep/x.rs"] {
        let node = find(&diff.entries, path);
        assert_eq!(node.status, ChangeStatus::Added, "{path}");
        assert!(node.has_changes);
    }
    assert_eq!(diff.stats.added, 3);
}

#[test]
fn everything_inside_a_deleted_folder_is_deleted() {
    let diff = compare(vec![dir("lib"), file("lib/x.rs", 3)], vec![]);

    assert_eq!(find(&diff.entries, "lib").status, ChangeStatus::Deleted);
    assert_eq!(
        find(&diff.entries, "lib/x.rs").status,
        ChangeStatus::Deleted
    );
}

#[test]
fn different_sizes_are_modified_without_hashing() {
    let comparer =
        FolderComparer::from_entries(vec![file("a.txt", 1)], vec![file("a.txt", 2)]).unwrap();
    assert!(comparer.jobs().is_empty());

    let node = &comparer.diff().entries[0];
    assert_eq!(node.status, ChangeStatus::Modified);
    assert_eq!(
        node.reasons,
        vec![ChangeReason::Content, ChangeReason::Size]
    );
}

#[test]
fn equal_sizes_are_hashed_on_both_sides() {
    let comparer =
        FolderComparer::from_entries(vec![file("a.txt", 4)], vec![file("a.txt", 4)]).unwrap();
    let jobs: Vec<(u32, Side, &str)> = comparer
        .jobs()
        .iter()
        .map(|job| (job.id, job.side, job.path.as_str()))
        .collect();

    assert_eq!(
        jobs,
        vec![(0, Side::Left, "a.txt"), (1, Side::Right, "a.txt")]
    );
}

#[test]
fn different_content_of_the_same_size_is_modified() {
    let diff = compare_with(vec![file("a.txt", 4)], vec![file("a.txt", 4)], |side, _| {
        Some(match side {
            Side::Left => b"abcd",
            Side::Right => b"abce",
        })
    });

    let node = &diff.entries[0];
    assert_eq!(node.status, ChangeStatus::Modified);
    assert_eq!(node.reasons, vec![ChangeReason::Content]);
}

#[test]
fn each_metadata_change_is_its_own_reason() {
    let cases: Vec<(FsEntry, ChangeReason)> = vec![
        (
            FsEntry {
                mode: 0o100_755,
                ..file("a", 4)
            },
            ChangeReason::Permissions,
        ),
        (
            FsEntry {
                uid: 0,
                ..file("a", 4)
            },
            ChangeReason::Owner,
        ),
        (
            FsEntry {
                gid: 0,
                ..file("a", 4)
            },
            ChangeReason::Owner,
        ),
        (
            FsEntry {
                mtime_ns: "1700000000000000001".to_string(),
                ..file("a", 4)
            },
            ChangeReason::ModifiedTime,
        ),
    ];

    for (right, reason) in cases {
        let diff = compare(vec![file("a", 4)], vec![right]);
        let node = &diff.entries[0];
        assert_eq!(node.status, ChangeStatus::Modified);
        assert_eq!(node.reasons, vec![reason]);
    }
}

#[test]
fn all_reasons_are_reported_together() {
    let right = FsEntry {
        mode: 0o100_600,
        uid: 0,
        mtime_ns: "5".to_string(),
        ..file("a", 9)
    };
    let diff = compare(vec![file("a", 4)], vec![right]);

    assert_eq!(
        diff.entries[0].reasons,
        vec![
            ChangeReason::Content,
            ChangeReason::Size,
            ChangeReason::Permissions,
            ChangeReason::Owner,
            ChangeReason::ModifiedTime,
        ]
    );
}

#[test]
fn equal_mtimes_compare_numerically() {
    let right = FsEntry {
        mtime_ns: "+1700000000000000000".to_string(),
        ..file("a", 4)
    };
    let diff = compare(vec![file("a", 4)], vec![right]);

    assert_eq!(diff.entries[0].status, ChangeStatus::Unchanged);
}

#[test]
fn symlinks_compare_their_target() {
    let diff = compare(
        vec![symlink("link", "a.txt")],
        vec![symlink("link", "b.txt")],
    );

    assert_eq!(diff.entries[0].reasons, vec![ChangeReason::LinkTarget]);
}

#[test]
fn folder_metadata_is_compared_but_not_its_size() {
    let right = FsEntry {
        size: 8192,
        mtime_ns: "1".to_string(),
        ..dir("src")
    };
    let diff = compare(vec![dir("src")], vec![right]);

    assert_eq!(diff.entries[0].status, ChangeStatus::Modified);
    assert_eq!(diff.entries[0].reasons, vec![ChangeReason::ModifiedTime]);
}

#[test]
fn a_change_inside_marks_unchanged_ancestors_as_having_changes() {
    let diff = compare(
        vec![dir("a"), dir("a/b"), file("a/b/c.txt", 1), dir("x")],
        vec![dir("a"), dir("a/b"), file("a/b/c.txt", 2), dir("x")],
    );

    for path in ["a", "a/b"] {
        let node = find(&diff.entries, path);
        assert_eq!(node.status, ChangeStatus::Unchanged, "{path}");
        assert!(node.has_changes, "{path}");
    }
    assert!(!find(&diff.entries, "x").has_changes);
    assert!(!diff.identical);
}

#[test]
fn a_file_replaced_by_a_folder_is_a_kind_change_with_children_from_both_sides() {
    let diff = compare(
        vec![file("thing", 4)],
        vec![dir("thing"), file("thing/inner.txt", 1)],
    );

    let node = find(&diff.entries, "thing");
    assert_eq!(node.status, ChangeStatus::Modified);
    assert_eq!(node.reasons, vec![ChangeReason::Kind]);
    assert_eq!(
        (node.left_kind, node.right_kind),
        (Some(EntryKind::File), Some(EntryKind::Dir))
    );
    assert_eq!(
        find(&diff.entries, "thing/inner.txt").status,
        ChangeStatus::Added
    );
}

#[test]
fn different_special_files_are_a_kind_change() {
    let fifo = FsEntry {
        kind: EntryKind::Other,
        mode: 0o010_644,
        ..file("pipe", 0)
    };
    let socket = FsEntry {
        mode: 0o140_644,
        ..fifo.clone()
    };
    let comparer = FolderComparer::from_entries(vec![fifo], vec![socket]).unwrap();
    assert!(comparer.jobs().is_empty(), "special files are never read");

    assert_eq!(comparer.diff().entries[0].reasons, vec![ChangeReason::Kind]);
}

#[test]
fn an_unreadable_entry_is_unknown() {
    let broken = FsEntry {
        error: Some("EACCES".to_string()),
        mtime_ns: String::new(),
        ..dir("secret")
    };
    let diff = compare(vec![dir("secret")], vec![broken]);

    let node = &diff.entries[0];
    assert_eq!(node.status, ChangeStatus::Unknown);
    assert_eq!(node.error.as_deref(), Some("right: EACCES"));
    assert_eq!(diff.stats.unknown, 1);
    assert!(!diff.identical);
}

#[test]
fn a_failed_read_is_unknown() {
    let diff = compare_with(vec![file("a", 4)], vec![file("a", 4)], |side, _| {
        (side == Side::Left).then_some(b"abcd".as_slice())
    });

    let node = &diff.entries[0];
    assert_eq!(node.status, ChangeStatus::Unknown);
    assert_eq!(node.error.as_deref(), Some("right: permission denied"));
}

#[test]
fn a_known_difference_wins_over_a_failed_read() {
    let right = FsEntry {
        uid: 0,
        ..file("a", 4)
    };
    let diff = compare_with(vec![file("a", 4)], vec![right], |_, _| None);

    let node = &diff.entries[0];
    assert_eq!(node.status, ChangeStatus::Modified);
    assert_eq!(node.reasons, vec![ChangeReason::Owner]);
    assert!(node.error.is_some());
}

#[test]
fn a_hash_that_was_never_set_is_unknown() {
    let comparer = FolderComparer::from_entries(vec![file("a", 4)], vec![file("a", 4)]).unwrap();

    let node = &comparer.diff().entries[0];
    assert_eq!(node.status, ChangeStatus::Unknown);
    assert_eq!(
        node.error.as_deref(),
        Some("left: content was not hashed; right: content was not hashed")
    );
}

#[test]
fn empty_folders_are_compared() {
    let diff = compare(vec![dir("empty")], vec![dir("empty")]);

    assert_eq!(diff.entries[0].status, ChangeStatus::Unchanged);
    assert!(diff.entries[0].children.is_empty());
}

#[test]
fn empty_listings_are_identical() {
    let diff = compare(vec![], vec![]);

    assert!(diff.entries.is_empty());
    assert!(diff.identical);
}

#[test]
fn children_are_sorted_folders_first_then_by_name_ignoring_case() {
    let listing = vec![
        file("b.txt", 1),
        dir("Zed"),
        file("A.txt", 1),
        dir("alpha"),
        file("a.txt", 1),
    ];
    let diff = compare(listing, vec![]);

    let names: Vec<&str> = diff.entries.iter().map(|node| node.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "Zed", "A.txt", "a.txt", "b.txt"]);
}

#[test]
fn hidden_and_tooling_folders_are_not_skipped() {
    let listing = || {
        vec![
            dir(".git"),
            file(".git/HEAD", 4),
            dir("node_modules"),
            file(".env", 4),
        ]
    };
    let diff = compare(listing(), listing());

    assert_eq!(diff.stats.unchanged, 4);
}

#[test]
fn chunked_hashing_matches_a_single_update() {
    let content = b"the quick brown fox jumps over the lazy dog".repeat(1000);

    let mut chunked = Hasher::new();
    for chunk in content.chunks(777) {
        chunked.update(chunk);
    }

    assert_eq!(chunked.finish(), hash(&content));
    assert_eq!(hash(b"").len(), 32);
    assert_ne!(hash(b"a"), hash(b"b"));
}

#[test]
fn rejects_invalid_listings() {
    let cases: Vec<(Vec<FsEntry>, &str)> = vec![
        (vec![file("", 1)], "path is empty"),
        (vec![file("/abs", 1)], "must be relative"),
        (vec![dir("a"), file("a//b", 1)], "must be relative"),
        (vec![file("a/", 1)], "must be relative"),
        (vec![dir("a"), file("a/../b", 1)], "must not contain"),
        (vec![file("a", 1), file("a", 1)], "listed more than once"),
        (vec![file("a/b", 1)], "is not listed"),
        (vec![file("a", 1), file("a/b", 1)], "is not a folder"),
        (
            vec![FsEntry {
                mtime_ns: "12.5".to_string(),
                ..file("a", 1)
            }],
            "mtime_ns must be an integer",
        ),
    ];

    for (listing, message) in cases {
        let error = FolderComparer::from_entries(vec![], listing)
            .err()
            .unwrap_or_else(|| panic!("expected an error containing {message:?}"));
        let error = error.to_string();
        assert!(error.starts_with("right entry"), "{error}");
        assert!(
            error.contains(message),
            "{error} should contain {message:?}"
        );
    }
}

#[test]
fn rejects_malformed_json_and_unknown_job_ids() {
    let error = FolderComparer::new("[]", "{").err().unwrap();
    assert!(error.to_string().starts_with("right listing is invalid"));

    let mut comparer = FolderComparer::new("[]", "[]").unwrap();
    assert!(comparer.set_hash(0, "abc").is_err());
    assert!(comparer.set_error(7, "nope").is_err());
}

#[test]
fn wasm_api_round_trips_json() {
    let entry =
        r#"{"path":"a.txt","kind":"file","size":4,"mode":33188,"uid":1,"gid":1,"mtime_ns":"10"}"#;
    let mut comparer = FolderComparer::new(&format!("[{entry}]"), &format!("[{entry}]")).unwrap();

    let jobs: serde_json::Value = serde_json::from_str(&comparer.pending_hashes()).unwrap();
    assert_eq!(
        jobs,
        serde_json::json!([
            { "id": 0, "side": "left", "path": "a.txt" },
            { "id": 1, "side": "right", "path": "a.txt" },
        ])
    );

    comparer.set_hash(0, &hash(b"abcd")).unwrap();
    comparer.set_hash(1, &hash(b"abcd")).unwrap();

    let diff: serde_json::Value = serde_json::from_str(&comparer.finish()).unwrap();
    assert_eq!(
        diff,
        serde_json::json!({
            "entries": [{
                "name": "a.txt",
                "path": "a.txt",
                "left_kind": "file",
                "right_kind": "file",
                "status": "unchanged",
                "reasons": [],
                "has_changes": false,
                "children": [],
            }],
            "stats": { "added": 0, "deleted": 0, "modified": 0, "unchanged": 1, "unknown": 0 },
            "identical": true,
        })
    );
}

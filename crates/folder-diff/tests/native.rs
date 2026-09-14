#![cfg(not(target_arch = "wasm32"))]

use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, SystemTime};

use folder_diff::{
    ChangeReason, ChangeStatus, DiffError, EntryKind, FolderComparer, FolderDiff, Hasher, Side,
    TreeNode, diff_folders, list_folder,
};
use tempfile::TempDir;

fn write(root: &Path, path: &str, content: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Gives every file and folder under `root` the same modification time, so only the
/// changes a test makes on purpose show up. Symlinks are left alone: setting a time
/// through one would change its target instead.
fn settle(root: &Path) {
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    for entry in list_folder(root).unwrap() {
        if entry.kind != EntryKind::Symlink {
            // Windows cannot open a folder as a file, so there only files are settled.
            if let Ok(file) = File::open(root.join(&entry.path)) {
                let _ = file.set_modified(time);
            }
        }
    }
    if let Ok(file) = File::open(root) {
        let _ = file.set_modified(time);
    }
}

fn folders() -> (TempDir, TempDir) {
    (TempDir::new().unwrap(), TempDir::new().unwrap())
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
fn finds_content_changes_on_disk() {
    let (left, right) = folders();
    for root in [left.path(), right.path()] {
        write(root, "src/same.txt", b"same");
        write(root, ".hidden/config", b"keep");
    }
    write(left.path(), "src/edit.txt", b"abcd");
    write(right.path(), "src/edit.txt", b"abce");
    write(left.path(), "src/grown.txt", b"short");
    write(right.path(), "src/grown.txt", b"much longer");
    write(left.path(), "gone.txt", b"bye");
    write(right.path(), "lib/new/index.rs", b"hello");
    settle(left.path());
    settle(right.path());

    let diff = diff_folders(left.path(), right.path()).unwrap();

    let status = |path| find(&diff.entries, path).status;
    assert_eq!(status("src/same.txt"), ChangeStatus::Unchanged);
    assert_eq!(status(".hidden/config"), ChangeStatus::Unchanged);
    assert_eq!(status("src/edit.txt"), ChangeStatus::Modified);
    assert_eq!(
        find(&diff.entries, "src/grown.txt").reasons,
        vec![ChangeReason::Content, ChangeReason::Size]
    );
    assert_eq!(status("gone.txt"), ChangeStatus::Deleted);
    assert_eq!(status("lib"), ChangeStatus::Added);
    assert_eq!(status("lib/new/index.rs"), ChangeStatus::Added);
    assert!(
        find(&diff.entries, "src/edit.txt")
            .reasons
            .contains(&ChangeReason::Content)
    );
    assert!(!diff.identical);
}

#[test]
fn identical_folders_on_disk_are_identical() {
    let (left, right) = folders();
    for root in [left.path(), right.path()] {
        write(root, "a/b/c.txt", b"deep");
        write(root, "top.txt", b"top");
    }
    settle(left.path());
    settle(right.path());

    let diff = diff_folders(left.path(), right.path()).unwrap();

    assert!(diff.identical, "{diff:#?}");
    assert_eq!(diff.stats.unchanged, 4);
}

#[test]
fn large_files_are_hashed_in_chunks() {
    let (left, right) = folders();
    let content = vec![7u8; 3 * 1024 * 1024 + 17];
    let mut changed = content.clone();
    *changed.last_mut().unwrap() = 8;
    for root in [left.path(), right.path()] {
        write(root, "same.bin", &content);
    }
    write(left.path(), "tail.bin", &content);
    write(right.path(), "tail.bin", &changed);
    settle(left.path());
    settle(right.path());

    let diff = diff_folders(left.path(), right.path()).unwrap();

    assert_eq!(
        find(&diff.entries, "same.bin").status,
        ChangeStatus::Unchanged
    );
    let tail = find(&diff.entries, "tail.bin");
    assert_eq!(tail.status, ChangeStatus::Modified);
    assert_eq!(tail.reasons, vec![ChangeReason::Content]);
}

/// The parallel listing and hashing must land on exactly the tree a plain,
/// one-file-at-a-time comparison of the same folders produces.
#[test]
fn parallel_comparison_matches_a_sequential_one() {
    let (left, right) = folders();
    for folder in 0..30 {
        for file in 0..50 {
            let path = format!("dir{folder}/sub{}/file{file}.txt", file % 4);
            let content = format!("{folder}-{file}");
            write(left.path(), &path, content.as_bytes());
            match (folder + file) % 7 {
                0 => write(right.path(), &path, format!("{folder}+{file}").as_bytes()),
                1 => {}
                _ => write(right.path(), &path, content.as_bytes()),
            }
        }
    }
    write(right.path(), "dir3/extra.txt", b"extra");
    settle(left.path());
    settle(right.path());

    let parallel = diff_folders(left.path(), right.path()).unwrap();

    let mut comparer = FolderComparer::new(
        list_folder(left.path()).unwrap(),
        list_folder(right.path()).unwrap(),
    )
    .unwrap();
    for job in comparer.jobs().to_vec() {
        let root = match job.side {
            Side::Left => left.path(),
            Side::Right => right.path(),
        };
        let mut hasher = Hasher::new();
        hasher.update(&fs::read(root.join(&job.path)).unwrap());
        comparer.set_hash(job.id, hasher.finish()).unwrap();
    }
    let sequential: FolderDiff = comparer.diff();

    assert_eq!(parallel, sequential);
    assert!(parallel.stats.modified > 0 && parallel.stats.deleted > 0);
    assert_eq!(parallel.stats.added, 1);
}

#[test]
fn a_missing_folder_fails_the_comparison() {
    let left = TempDir::new().unwrap();
    let missing = left.path().join("does-not-exist");

    let error = diff_folders(left.path(), &missing).unwrap_err();

    assert!(matches!(
        error,
        DiffError::Unreadable {
            side: Side::Right,
            ..
        }
    ));
    assert!(
        error
            .to_string()
            .starts_with("right folder could not be listed")
    );
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;

    #[test]
    fn metadata_changes_are_reasons() {
        let (left, right) = folders();
        for root in [left.path(), right.path()] {
            write(root, "run.sh", b"#!/bin/sh");
            write(root, "notes.txt", b"notes");
        }
        fs::set_permissions(
            right.path().join("run.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        settle(left.path());
        settle(right.path());
        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        File::open(right.path().join("notes.txt"))
            .unwrap()
            .set_modified(later)
            .unwrap();

        let diff = diff_folders(left.path(), right.path()).unwrap();

        assert_eq!(
            find(&diff.entries, "run.sh").reasons,
            vec![ChangeReason::Permissions]
        );
        assert_eq!(
            find(&diff.entries, "notes.txt").reasons,
            vec![ChangeReason::ModifiedTime]
        );
    }

    #[test]
    fn symlinks_are_listed_but_never_followed() {
        let (left, right) = folders();
        for root in [left.path(), right.path()] {
            write(root, "real/file.txt", b"real");
            // A link back to its own folder would loop forever if it were followed.
            symlink(".", root.join("real/loop")).unwrap();
        }
        symlink("real/file.txt", left.path().join("link")).unwrap();
        symlink("elsewhere.txt", right.path().join("link")).unwrap();

        let listing = list_folder(left.path()).unwrap();
        let link = listing
            .iter()
            .find(|entry| entry.path == "real/loop")
            .unwrap();
        assert_eq!(link.kind, EntryKind::Symlink);
        assert_eq!(link.link_target.as_deref(), Some("."));
        assert!(
            !listing
                .iter()
                .any(|entry| entry.path.starts_with("real/loop/"))
        );

        let diff = diff_folders(left.path(), right.path()).unwrap();
        assert!(
            find(&diff.entries, "link")
                .reasons
                .contains(&ChangeReason::LinkTarget)
        );
    }

    #[test]
    fn an_unreadable_folder_is_unknown_rather_than_fatal() {
        let (left, right) = folders();
        for root in [left.path(), right.path()] {
            write(root, "locked/secret.txt", b"secret");
            write(root, "open.txt", b"open");
        }
        settle(left.path());
        settle(right.path());
        let locked = right.path().join("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        // Permissions do not stop root, so there is nothing to test when running as it.
        let readable = fs::read_dir(&locked).is_ok();

        let diff = diff_folders(left.path(), right.path());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        if readable {
            return;
        }

        let diff = diff.unwrap();
        let node = find(&diff.entries, "locked");
        assert_eq!(node.status, ChangeStatus::Unknown);
        assert!(node.error.as_deref().unwrap().starts_with("right: "));
        assert_eq!(
            find(&diff.entries, "open.txt").status,
            ChangeStatus::Unchanged
        );
    }
}

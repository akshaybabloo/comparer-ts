//! The WebAssembly-facing folder API: JSON in, JSON out. The comparison itself is
//! tested in `folder-diff`.

use comparer::{FolderComparer, Hasher};

fn hash(content: &[u8]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(content);
    hasher.finish()
}

#[test]
fn round_trips_json() {
    let entry =
        r#"{"path":"a.txt","kind":"file","size":4,"mode":33188,"uid":1,"gid":1,"mtime_ns":"10"}"#;
    let mut comparer = FolderComparer::new(&format!("[{entry}]"), &format!("[{entry}]")).unwrap();

    let jobs: serde_json::Value = serde_json::from_str(&comparer.pending_hashes()).unwrap();
    assert_eq!(
        jobs,
        serde_json::json!([
            { "id": 0, "side": "left", "path": "a.txt", "size": 4 },
            { "id": 1, "side": "right", "path": "a.txt", "size": 4 },
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

#[test]
fn optional_fields_may_be_null_or_missing() {
    let missing =
        r#"[{"path":"l","kind":"symlink","size":1,"mode":41471,"uid":1,"gid":1,"mtime_ns":"1"}]"#;
    let null = r#"[{"path":"l","kind":"symlink","size":1,"mode":41471,"uid":1,"gid":1,"mtime_ns":"1","link_target":null,"error":null}]"#;

    let diff: serde_json::Value =
        serde_json::from_str(&FolderComparer::new(missing, null).unwrap().finish()).unwrap();

    assert_eq!(diff["identical"], true);
}

#[test]
fn reports_errors_with_their_messages() {
    let error = FolderComparer::new("[]", "{").err().unwrap();
    assert!(error.to_string().starts_with("right listing is invalid"));

    let invalid =
        r#"[{"path":"a/b","kind":"file","size":1,"mode":1,"uid":1,"gid":1,"mtime_ns":"1"}]"#;
    let error = FolderComparer::new(invalid, "[]").err().unwrap();
    assert_eq!(
        error.to_string(),
        r#"left entry "a": has entries inside it but is not listed"#
    );

    let mut comparer = FolderComparer::new("[]", "[]").unwrap();
    assert_eq!(
        comparer.set_hash(0, "abc").unwrap_err().to_string(),
        "unknown hash job 0"
    );
    assert!(comparer.set_error(7, "nope").is_err());
}

#[test]
fn chunked_hashing_matches_a_single_update() {
    let content = b"the quick brown fox jumps over the lazy dog".repeat(1000);

    let mut chunked = Hasher::new();
    for chunk in content.chunks(777) {
        chunked.update(chunk);
    }

    assert_eq!(chunked.finish(), hash(&content));
}

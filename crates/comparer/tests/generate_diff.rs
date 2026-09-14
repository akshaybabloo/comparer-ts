//! The WebAssembly-facing text API: the exact JSON it hands to JavaScript. The diffing
//! itself is tested in `text-diff`.

use comparer::{generate_diff, generate_inline_diff};
use serde_json::{Value, json};

fn parse(json: String) -> Value {
    serde_json::from_str(&json).unwrap()
}

#[test]
fn line_diff_json_has_the_documented_shape() {
    assert_eq!(
        parse(generate_diff("hello\nworld\n", "hello\nthere\n")),
        json!([
            { "tag": "equal", "old_line": 0, "new_line": 0, "value": "hello", "missing_newline": false },
            { "tag": "delete", "old_line": 1, "new_line": null, "value": "world", "missing_newline": false },
            { "tag": "insert", "old_line": null, "new_line": 1, "value": "there", "missing_newline": false },
        ])
    );
}

#[test]
fn inline_diff_json_has_the_documented_shape() {
    assert_eq!(
        parse(generate_inline_diff("hello world\n", "hello there\n")),
        json!([
            {
                "tag": "delete",
                "old_line": 0,
                "new_line": null,
                "segments": [
                    { "emphasized": false, "value": "hello " },
                    { "emphasized": true, "value": "world" },
                ],
                "missing_newline": false,
            },
            {
                "tag": "insert",
                "old_line": null,
                "new_line": 0,
                "segments": [
                    { "emphasized": false, "value": "hello " },
                    { "emphasized": true, "value": "there" },
                ],
                "missing_newline": false,
            },
        ])
    );
}

#[test]
fn empty_inputs_give_empty_arrays() {
    assert_eq!(generate_diff("", ""), "[]");
    assert_eq!(generate_inline_diff("", ""), "[]");
}

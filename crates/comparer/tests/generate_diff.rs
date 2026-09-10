use comparer::{generate_diff, generate_inline_diff};
use serde_json::Value;

#[test]
fn generate_diff_marks_equal_lines() {
    let result = generate_diff("a\nb\nc\n", "a\nb\nc\n");
    let lines: Value = serde_json::from_str(&result).unwrap();
    let lines = lines.as_array().unwrap();

    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|line| line["tag"] == "equal"));
}

#[test]
fn generate_diff_marks_inserted_and_deleted_lines() {
    let result = generate_diff("a\nb\n", "a\nc\n");
    let lines: Value = serde_json::from_str(&result).unwrap();
    let lines = lines.as_array().unwrap();

    let tags: Vec<&str> = lines
        .iter()
        .map(|line| line["tag"].as_str().unwrap())
        .collect();
    assert_eq!(tags, vec!["equal", "delete", "insert"]);
    assert_eq!(lines[1]["value"], "b");
    assert_eq!(lines[2]["value"], "c");
}

#[test]
fn generate_diff_returns_empty_array_for_empty_inputs() {
    let result = generate_diff("", "");
    assert_eq!(result, "[]");
}

#[test]
fn generate_diff_strips_crlf_line_endings() {
    let result = generate_diff("a\r\nb\r\n", "a\r\nc\r\n");
    let lines: Value = serde_json::from_str(&result).unwrap();
    let lines = lines.as_array().unwrap();

    let values: Vec<&str> = lines
        .iter()
        .map(|line| line["value"].as_str().unwrap())
        .collect();
    assert_eq!(values, vec!["a", "b", "c"]);
}

#[test]
fn generate_diff_keeps_carriage_return_that_is_not_a_line_ending() {
    // No trailing newline, so the `\r` is content rather than half a terminator.
    let result = generate_diff("a\r", "b\r");
    let lines: Value = serde_json::from_str(&result).unwrap();
    let lines = lines.as_array().unwrap();

    assert_eq!(lines[0]["value"], "a\r");
    assert_eq!(lines[1]["value"], "b\r");
}

#[test]
fn generate_diff_reports_missing_trailing_newline() {
    let result = generate_diff("a", "a\n");
    let lines: Value = serde_json::from_str(&result).unwrap();
    let lines = lines.as_array().unwrap();

    // Both sides read "a"; `missing_newline` is the only thing telling them apart.
    assert_eq!(lines[0]["tag"], "delete");
    assert_eq!(lines[0]["value"], "a");
    assert_eq!(lines[0]["missing_newline"], true);

    assert_eq!(lines[1]["tag"], "insert");
    assert_eq!(lines[1]["value"], "a");
    assert_eq!(lines[1]["missing_newline"], false);
}

#[test]
fn generate_inline_diff_marks_equal_lines() {
    let result = generate_inline_diff("a\nb\nc\n", "a\nb\nc\n");
    let lines: Value = serde_json::from_str(&result).unwrap();
    let lines = lines.as_array().unwrap();

    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|line| line["tag"] == "equal"));
}

#[test]
fn generate_inline_diff_highlights_changed_characters() {
    let result = generate_inline_diff("hello world\n", "hello there\n");
    let lines: Value = serde_json::from_str(&result).unwrap();
    let lines = lines.as_array().unwrap();

    let delete_line = lines.iter().find(|line| line["tag"] == "delete").unwrap();
    let segments = delete_line["segments"].as_array().unwrap();
    assert!(segments.iter().any(|s| s["emphasized"] == true));
}

#[test]
fn generate_inline_diff_returns_empty_array_for_empty_inputs() {
    let result = generate_inline_diff("", "");
    assert_eq!(result, "[]");
}

#[test]
fn generate_inline_diff_emits_no_empty_segments() {
    let result = generate_inline_diff("hello world\n", "hello there\n");
    let lines: Value = serde_json::from_str(&result).unwrap();

    for line in lines.as_array().unwrap() {
        for segment in line["segments"].as_array().unwrap() {
            assert_ne!(
                segment["value"], "",
                "empty segment leaked into {}",
                line["tag"]
            );
        }
    }
}

#[test]
fn generate_inline_diff_strips_crlf_line_endings() {
    let result = generate_inline_diff("hello world\r\n", "hello there\r\n");
    let lines: Value = serde_json::from_str(&result).unwrap();

    for line in lines.as_array().unwrap() {
        for segment in line["segments"].as_array().unwrap() {
            let value = segment["value"].as_str().unwrap();
            assert!(
                !value.contains('\r'),
                "carriage return leaked into {value:?}"
            );
        }
    }
}

#[test]
fn generate_inline_diff_drops_segments_for_a_blank_line() {
    let result = generate_inline_diff("a\n\nb\n", "a\n\nb\n");
    let lines: Value = serde_json::from_str(&result).unwrap();
    let lines = lines.as_array().unwrap();

    // The blank line has nothing left once its terminator is stripped.
    assert_eq!(lines[1]["segments"].as_array().unwrap().len(), 0);
}

#[test]
fn generate_inline_diff_reports_missing_trailing_newline() {
    let result = generate_inline_diff("a", "a\n");
    let lines: Value = serde_json::from_str(&result).unwrap();
    let lines = lines.as_array().unwrap();

    assert_eq!(lines[0]["tag"], "delete");
    assert_eq!(lines[0]["missing_newline"], true);
    assert_eq!(lines[1]["tag"], "insert");
    assert_eq!(lines[1]["missing_newline"], false);
}

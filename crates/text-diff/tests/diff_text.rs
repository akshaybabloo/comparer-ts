use similar::TextDiff;
use text_diff::{LineTag, Segment, diff_inline, diff_lines};

fn tags(old: &str, new: &str) -> Vec<LineTag> {
    diff_lines(old, new).iter().map(|line| line.tag).collect()
}

#[test]
fn equal_lines_are_equal() {
    assert_eq!(tags("a\nb\nc\n", "a\nb\nc\n"), vec![LineTag::Equal; 3]);
}

#[test]
fn changed_lines_are_deleted_and_inserted() {
    let lines = diff_lines("a\nb\n", "a\nc\n");

    assert_eq!(
        lines.iter().map(|line| line.tag).collect::<Vec<_>>(),
        vec![LineTag::Equal, LineTag::Delete, LineTag::Insert]
    );
    assert_eq!((lines[1].old_line, lines[1].new_line), (Some(1), None));
    assert_eq!((lines[2].old_line, lines[2].new_line), (None, Some(1)));
    assert_eq!(lines[1].value, "b");
    assert_eq!(lines[2].value, "c");
}

#[test]
fn empty_texts_have_no_lines() {
    assert!(diff_lines("", "").is_empty());
    assert!(diff_inline("", "").is_empty());
}

#[test]
fn crlf_line_endings_are_stripped() {
    let values: Vec<String> = diff_lines("a\r\nb\r\n", "a\r\nc\r\n")
        .into_iter()
        .map(|line| line.value)
        .collect();

    assert_eq!(values, vec!["a", "b", "c"]);
}

#[test]
fn a_carriage_return_that_is_not_a_line_ending_is_kept() {
    // No trailing newline, so the `\r` is content rather than half a terminator.
    let lines = diff_lines("a\r", "b\r");

    assert_eq!(lines[0].value, "a\r");
    assert_eq!(lines[1].value, "b\r");
}

#[test]
fn a_missing_trailing_newline_is_reported() {
    let lines = diff_lines("a", "a\n");

    // Both sides read "a"; `missing_newline` is the only thing telling them apart.
    assert_eq!(
        (lines[0].tag, lines[0].value.as_str()),
        (LineTag::Delete, "a")
    );
    assert!(lines[0].missing_newline);
    assert_eq!(
        (lines[1].tag, lines[1].value.as_str()),
        (LineTag::Insert, "a")
    );
    assert!(!lines[1].missing_newline);

    let inline = diff_inline("a", "a\n");
    assert!(inline[0].missing_newline);
    assert!(!inline[1].missing_newline);
}

#[test]
fn inline_equal_lines_are_equal() {
    let lines = diff_inline("a\nb\nc\n", "a\nb\nc\n");

    assert!(lines.iter().all(|line| line.tag == LineTag::Equal));
    assert_eq!(lines.len(), 3);
}

#[test]
fn inline_emphasizes_only_the_changed_words() {
    let lines = diff_inline("hello world\n", "hello there\n");

    let deleted = lines
        .iter()
        .find(|line| line.tag == LineTag::Delete)
        .unwrap();
    assert_eq!(
        deleted.segments,
        vec![
            Segment {
                emphasized: false,
                value: "hello ".to_string()
            },
            Segment {
                emphasized: true,
                value: "world".to_string()
            },
        ]
    );
}

#[test]
fn inline_segments_are_never_empty_and_carry_no_line_endings() {
    for (old, new) in [
        ("hello world\n", "hello there\n"),
        ("hello world\r\n", "hello there\r\n"),
    ] {
        for line in diff_inline(old, new) {
            for segment in line.segments {
                assert_ne!(segment.value, "");
                assert!(!segment.value.contains(['\r', '\n']), "{segment:?}");
            }
        }
    }
}

#[test]
fn inline_blank_lines_have_no_segments() {
    let lines = diff_inline("a\n\nb\n", "a\n\nb\n");

    assert!(lines[1].segments.is_empty());
}

/// A text large enough to split into thousands of operations, so rayon really does
/// run them on several threads and has to put the rows back in order.
fn large_texts() -> (String, String) {
    let mut old = String::new();
    let mut new = String::new();
    for i in 0..20_000 {
        let line = format!("line {i} with some words in it\n");
        old.push_str(&line);
        match i % 7 {
            0 => new.push_str(&format!("line {i} with other words in it\n")),
            3 => {}
            5 => {
                new.push_str(&line);
                new.push_str(&format!("inserted after {i}\n"));
            }
            _ => new.push_str(&line),
        }
    }
    (old, new)
}

fn strip(value: &str) -> &str {
    let value = value.strip_suffix('\n').unwrap_or(value);
    value.strip_suffix('\r').unwrap_or(value)
}

#[test]
fn line_rows_match_a_sequential_walk_of_the_diff() {
    let (old, new) = large_texts();
    let diff = TextDiff::from_lines(&old, &new);
    assert!(diff.ops().len() > 1_000);

    let expected: Vec<_> = diff
        .iter_all_changes()
        .map(|change| {
            (
                LineTag::from(change.tag()),
                change.old_index(),
                change.new_index(),
                strip(change.value()).to_string(),
            )
        })
        .collect();
    let actual: Vec<_> = diff_lines(&old, &new)
        .into_iter()
        .map(|line| (line.tag, line.old_line, line.new_line, line.value))
        .collect();

    assert_eq!(actual, expected);
}

#[test]
fn inline_rows_match_a_sequential_walk_of_the_diff() {
    let (old, new) = large_texts();
    let diff = TextDiff::from_lines(&old, &new);

    let expected: Vec<_> = diff
        .iter_all_inline_changes()
        .map(|change| {
            let segments: Vec<(bool, String)> = change
                .iter_strings_lossy()
                .map(|(emphasized, value)| (emphasized, strip(&value).to_string()))
                .filter(|(_, value)| !value.is_empty())
                .collect();
            (
                LineTag::from(change.tag()),
                change.old_index(),
                change.new_index(),
                segments,
            )
        })
        .collect();
    let actual: Vec<_> = diff_inline(&old, &new)
        .into_iter()
        .map(|line| {
            let segments = line
                .segments
                .into_iter()
                .map(|segment| (segment.emphasized, segment.value))
                .collect();
            (line.tag, line.old_line, line.new_line, segments)
        })
        .collect();

    assert_eq!(actual, expected);
}

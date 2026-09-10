import { describe, expect, it } from "vitest";
import { generateDiff, generateInlineDiff } from "./index";

describe("generateDiff", () => {
  it("returns equal lines for identical input", () => {
    const result = generateDiff("hello\nworld\n", "hello\nworld\n");

    expect(result).toMatchObject([
      { tag: "equal", old_line: 0, new_line: 0, value: "hello" },
      { tag: "equal", old_line: 1, new_line: 1, value: "world" },
    ]);
  });

  it("returns insert and delete operations for differing text", () => {
    const result = generateDiff("a\n", "b\na\n");

    expect(result).toHaveLength(2);
    expect(result[0]).toMatchObject({
      tag: "insert",
      old_line: null,
      new_line: 0,
      value: "b",
    });
    expect(result[1]).toMatchObject({
      tag: "equal",
      old_line: 0,
      new_line: 1,
      value: "a",
    });
  });

  it("strips CRLF line endings instead of leaving a stray carriage return", () => {
    const result = generateDiff("a\r\nb\r\n", "a\r\nc\r\n");

    expect(result.map((line) => line.value)).toEqual(["a", "b", "c"]);
  });

  it("flags a missing trailing newline", () => {
    const result = generateDiff("a", "a\n");

    // Both rows read "a", so `missing_newline` is the only signal that anything changed.
    expect(result).toMatchObject([
      { tag: "delete", value: "a", missing_newline: true },
      { tag: "insert", value: "a", missing_newline: false },
    ]);
  });
});

describe("generateInlineDiff", () => {
  it("returns inline segment metadata for changed text", () => {
    const result = generateInlineDiff("hello world\n", "hello there\n");

    expect(Array.isArray(result)).toBe(true);
    expect(result.length).toBeGreaterThan(0);
    expect(result).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          tag: expect.any(String),
          segments: expect.any(Array),
        }),
      ]),
    );
  });

  it("emphasizes only the part of the line that changed", () => {
    const result = generateInlineDiff("hello world\n", "hello there\n");

    const deleted = result.find((line) => line.tag === "delete");
    expect(deleted?.segments).toEqual([
      { emphasized: false, value: "hello " },
      { emphasized: true, value: "world" },
    ]);
  });

  it("never emits empty segments", () => {
    const result = generateInlineDiff("hello world\r\n", "hello there\r\n");

    for (const line of result) {
      for (const segment of line.segments) {
        expect(segment.value).not.toBe("");
        expect(segment.value).not.toContain("\r");
      }
    }
  });

  it("returns an empty array for empty input", () => {
    expect(generateInlineDiff("", "")).toEqual([]);
  });
});

import { describe, expect, it } from "vitest";
import { Hasher } from "../crates/comparer/pkg/comparer.js";
import {
  compareFolders,
  generateDiff,
  generateInlineDiff,
  type FsEntry,
  type HashProgress,
  type ReadFile,
  type Side,
} from "./index";

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

describe("Hasher", () => {
  it("produces the same digest however the content is chunked", () => {
    const content = new TextEncoder().encode("the quick brown fox ".repeat(500));

    const whole = new Hasher();
    whole.update(content);

    const chunked = new Hasher();
    for (let i = 0; i < content.length; i += 333) chunked.update(content.subarray(i, i + 333));

    expect(chunked.finish()).toBe(whole.finish());
  });
});

describe("compareFolders", () => {
  const file = (path: string, size: number, overrides: Partial<FsEntry> = {}): FsEntry => ({
    path,
    kind: "file",
    size,
    mode: 0o100644,
    uid: 1000,
    gid: 1000,
    mtime_ns: "1700000000000000000",
    ...overrides,
  });
  const dir = (path: string): FsEntry => file(path, 4096, { kind: "dir", mode: 0o40755 });

  const reader = (contents: Record<Side, Record<string, string>>): ReadFile =>
    async function* (side, path) {
      const text = contents[side][path];
      if (text === undefined) throw new Error(`EACCES: ${path}`);
      const bytes = new TextEncoder().encode(text);
      // Two chunks, to exercise incremental hashing.
      yield bytes.subarray(0, 2);
      yield bytes.subarray(2);
    };

  it("classifies added, deleted, modified and unchanged entries", async () => {
    const left = [dir("src"), file("src/same.ts", 4), file("src/edit.ts", 4), file("gone.txt", 1)];
    const right = [
      dir("src"),
      file("src/same.ts", 4),
      file("src/edit.ts", 4),
      file("src/new.ts", 2),
      file("run.sh", 1, { mode: 0o100755 }),
    ];
    const read = reader({
      left: { "src/same.ts": "same", "src/edit.ts": "abcd" },
      right: { "src/same.ts": "same", "src/edit.ts": "abce" },
    });

    const diff = await compareFolders(left, right, read);

    expect(diff.identical).toBe(false);
    expect(diff.stats).toEqual({ added: 2, deleted: 1, modified: 1, unchanged: 2, unknown: 0 });
    expect(diff.entries.map((node) => [node.name, node.status])).toEqual([
      ["src", "unchanged"],
      ["gone.txt", "deleted"],
      ["run.sh", "added"],
    ]);

    const src = diff.entries[0];
    expect(src.has_changes).toBe(true);
    expect(src.children.map((node) => [node.name, node.status, node.reasons])).toEqual([
      ["edit.ts", "modified", ["content"]],
      ["new.ts", "added", []],
      ["same.ts", "unchanged", []],
    ]);
  });

  it("reports a file that fails to read as unknown", async () => {
    const listing = [file("locked", 4)];
    const read = reader({ left: { locked: "abcd" }, right: {} });

    const diff = await compareFolders(listing, listing, read);

    expect(diff.entries[0]).toMatchObject({
      status: "unknown",
      error: "right: EACCES: locked",
    });
  });

  it("reports progress for every hashed file", async () => {
    const listing = [file("a", 4), file("b", 4)];
    const read = reader({ left: { a: "aaaa", b: "bbbb" }, right: { a: "aaaa", b: "bbbb" } });
    const updates: HashProgress[] = [];

    await compareFolders(listing, listing, read, { onProgress: (p) => updates.push(p) });

    expect(updates).toHaveLength(4);
    expect(updates.at(-1)).toEqual({ done: 4, total: 4, bytes: 16 });
  });

  it("rejects when aborted", async () => {
    const listing = [file("a", 4)];
    const controller = new AbortController();
    const read: ReadFile = async function* () {
      controller.abort(new Error("cancelled"));
      yield new Uint8Array(4);
    };

    await expect(compareFolders(listing, listing, read, { signal: controller.signal })).rejects.toThrow("cancelled");
  });

  it("rejects an invalid listing", async () => {
    await expect(compareFolders([file("a/b", 1)], [], reader({ left: {}, right: {} }))).rejects.toThrow(
      'left entry "a": has entries inside it but is not listed',
    );
  });
});

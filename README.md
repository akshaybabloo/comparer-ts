# comparer-ts

A TypeScript library for diffing strings, folders and images, powered by WebAssembly.

Text diffing is done in Rust by [similar](https://github.com/mitsuhiko/similar), and folder and image diffing in Rust too, all compiled to WebAssembly, so it stays fast on large inputs while the public API remains plain TypeScript.

## Install

```sh
npm install comparer-ts
```

The WebAssembly module is inlined into the bundle, so there is no separate `.wasm` asset to host or configure.

## Usage

### Line diff

`generateDiff` compares two strings line by line.

```ts
import { generateDiff } from "comparer-ts";

generateDiff("hello\nworld\n", "hello\nthere\n");
```

```json
[
  { "tag": "equal", "old_line": 0, "new_line": 0, "value": "hello", "missing_newline": false },
  { "tag": "delete", "old_line": 1, "new_line": null, "value": "world", "missing_newline": false },
  { "tag": "insert", "old_line": null, "new_line": 1, "value": "there", "missing_newline": false }
]
```

| Field             | Description                                                                           |
| ----------------- | ------------------------------------------------------------------------------------- |
| `tag`             | `"equal"`, `"delete"` or `"insert"`.                                                  |
| `old_line`        | Zero-based line number in the first string, or `null` for inserted lines.             |
| `new_line`        | Zero-based line number in the second string, or `null` for deleted lines.             |
| `value`           | The line, with its trailing `\n` or `\r\n` removed.                                   |
| `missing_newline` | `true` when the source line had no trailing newline. See [below](#trailing-newlines). |

### Inline diff

`generateInlineDiff` returns the same rows, but splits each line into segments marking which parts actually changed, for word-level highlighting.

```ts
import { generateInlineDiff } from "comparer-ts";

generateInlineDiff("hello world\n", "hello there\n");
```

```json
[
  {
    "tag": "delete",
    "old_line": 0,
    "new_line": null,
    "segments": [
      { "emphasized": false, "value": "hello " },
      { "emphasized": true, "value": "world" }
    ],
    "missing_newline": false
  },
  {
    "tag": "insert",
    "old_line": null,
    "new_line": 0,
    "segments": [
      { "emphasized": false, "value": "hello " },
      { "emphasized": true, "value": "there" }
    ],
    "missing_newline": false
  }
]
```

Render `emphasized` segments with a highlight and the rest plainly. Segments are never empty, and a blank line simply has no segments at all.

This runs a second diff pass within each changed line, so it costs more than `generateDiff` — prefer the plain line diff when you do not need highlighting.

### Trailing newlines

Line values never include their terminator, so a file that ends without a newline would otherwise be indistinguishable from one that ends with it:

```ts
generateDiff("a", "a\n");
// [{ tag: "delete", value: "a", missing_newline: true },
//  { tag: "insert", value: "a", missing_newline: false }]
```

Both rows read `"a"`; `missing_newline` is the only thing that tells them apart. Use it to render the usual `\ No newline at end of file` marker.

`\r\n` line endings are stripped along with `\n`, so Windows input does not leave a stray carriage return at the end of every value. A `\r` that is not part of a terminator — at the end of a file with no final newline — is kept as content.

### Folder diff

`compareFolders` compares two folder listings and returns one merged tree with a status for every entry. The library never touches the filesystem: you list both folders and stream file content on request, and the matching, content hashing (XXH3-128) and tree building all run in WebAssembly.

```ts
import { createReadStream } from "node:fs";
import { join } from "node:path";
import { compareFolders } from "comparer-ts";

const roots = { left: "/path/to/original", right: "/path/to/changed" };

const diff = await compareFolders(leftEntries, rightEntries, (side, path, signal) =>
  createReadStream(join(roots[side], path), { signal }),
);
```

Each listing is an array of `FsEntry`, one per `lstat` result, for **every** entry under the folder — nested folders included:

| Field         | Description                                                                         |
| ------------- | ----------------------------------------------------------------------------------- |
| `path`        | Relative, `/`-separated, with no `.` or `..` segments. Case-sensitive.              |
| `kind`        | `"file"`, `"dir"`, `"symlink"` or `"other"` (FIFOs, sockets, devices).              |
| `size`        | Size in bytes.                                                                      |
| `mode`        | The full `st_mode`, type bits included.                                             |
| `uid`, `gid`  | Owning user and group.                                                              |
| `mtime_ns`    | Modification time in nanoseconds, as a decimal string (`stats.mtimeNs.toString()`). |
| `link_target` | Where a symlink points. Optional.                                                   |
| `error`       | Why the entry could not be read. Optional; its metadata is then not compared.       |

Only files present on both sides with equal sizes are read — a size difference already proves the content changed. `other` entries are never read. A file whose read throws is reported as `unknown` instead of failing the comparison.

The result is a `FolderDiff`:

```json
{
  "entries": [
    {
      "name": "src",
      "path": "src",
      "left_kind": "dir",
      "right_kind": "dir",
      "status": "unchanged",
      "reasons": [],
      "has_changes": true,
      "children": [
        {
          "name": "main.ts",
          "path": "src/main.ts",
          "left_kind": "file",
          "right_kind": "file",
          "status": "modified",
          "reasons": ["content", "modified_time"],
          "has_changes": true,
          "children": []
        }
      ]
    }
  ],
  "stats": { "added": 0, "deleted": 0, "modified": 1, "unchanged": 1, "unknown": 0 },
  "identical": false
}
```

| `status`    | Meaning                                                                                    |
| ----------- | ------------------------------------------------------------------------------------------ |
| `added`     | Only in the right folder. Everything inside an added folder is added too.                  |
| `deleted`   | Only in the left folder. Everything inside a deleted folder is deleted too.                |
| `modified`  | In both, but differs in at least one way, listed in `reasons`.                             |
| `unchanged` | In both, and identical in every compared property.                                         |
| `unknown`   | In both, but one side could not be read, so the difference cannot be settled. See `error`. |

`reasons` can contain `kind`, `content`, `size`, `permissions`, `owner`, `modified_time` and `link_target`. Access time and change time are not compared: reading a file updates the first, and no copy can preserve the second. A folder's own `size` is not compared either, since it is filesystem bookkeeping. A folder's status covers its own metadata only; `has_changes` says whether anything inside it changed. Children are sorted folders first, then by name ignoring case.

Pass `{ signal }` to cancel, `{ onProgress }` to receive `{ done, total, bytes }` after each file, and `{ concurrency }` to change how many files are read at once (default 8).

### Image diff

`compareImages` compares two images pixel by pixel. They are decoded in WebAssembly, can be PNG, JPEG, WebP, GIF or BMP — not necessarily the same format — and must be the same size.

```ts
import { readFile, writeFile } from "node:fs/promises";
import { compareImages } from "comparer-ts";

const diff = compareImages(await readFile("before.png"), await readFile("after.png"), {
  tolerance: 5,
  diffPng: true,
});

if (!diff.identical) await writeFile("diff.png", diff.diff_png!);
```

```json
{
  "width": 1920,
  "height": 1080,
  "different_pixels": 5184,
  "total_pixels": 2073600,
  "percent": 0.25,
  "identical": false,
  "diff_rgba": null,
  "diff_png": "<Uint8Array of PNG bytes>"
}
```

In a browser, `compareImagesRgba` takes raw RGBA pixels instead, such as a canvas's `ImageData.data`, and skips decoding altogether:

```ts
const diff = compareImagesRgba(before.data, after.data, before.width, before.height, { diffRgba: true });

context.putImageData(new ImageData(diff.diff_rgba!, diff.width, diff.height), 0, 0);
```

| Option      | Description                                                                |
| ----------- | -------------------------------------------------------------------------- |
| `tolerance` | How much colour difference to tolerate, from `0` (the default) to `100`.   |
| `diffRgba`  | Paint the diff image as raw RGBA pixels, in `diff_rgba`. Otherwise `null`. |
| `diffPng`   | Paint the diff image as PNG bytes, in `diff_png`. Otherwise `null`.        |

The diff image paints each differing pixel red and the rest of the left image faded to a light grey. Ask for both forms at once and the pixels are only compared once.

Pixels are compared by their difference in the YIQ colour space, the measure [pixelmatch](https://github.com/mapbox/pixelmatch) uses, which weights brightness over hue roughly as the eye does. Transparent pixels are blended onto white first.

| `tolerance` | What is tolerated                                                                       |
| ----------- | --------------------------------------------------------------------------------------- |
| `0`         | Nothing: any change to a pixel's bytes counts, even to an invisible, transparent pixel. |
| `1`         | A one-step change in a single channel.                                                  |
| `5`–`10`    | Small shifts in shade, of the kind JPEG compression and anti-aliasing leave behind.     |
| `97`        | Almost anything, black against white included.                                          |
| `100`       | Every change.                                                                           |

A pixel counts as different when its YIQ difference is over `(tolerance / 100)²` of the largest possible one, so the scale is finer at the low end, where tolerances are usually set.

In Rust the comparison runs on every core, one band of rows per thread. WebAssembly has no threads without cross-origin isolation and nightly Rust, so there it runs on one: a 4K frame takes about 15–65 ms to count, depending on how much changed, and painting a diff image adds about 70 ms.

### Types

The result and listing types are generated from the Rust structs by [ts-rs](https://github.com/Aleph-Alpha/ts-rs) and exported from the package root:

```ts
import type {
  ChangeReason,
  ChangeStatus,
  EntryKind,
  FolderDiff,
  FolderStats,
  FsEntry,
  InlineLineDiff,
  LineDiff,
  LineTag,
  Segment,
  TreeNode,
} from "comparer-ts";
```

The image diff types hold pixel buffers, which ts-rs cannot describe, so they are written in TypeScript and exported alongside:

```ts
import type { CompareImagesOptions, ImageDiff } from "comparer-ts";
```

## Development

Requires [Rust](https://rustup.rs), [wasm-pack](https://rustwasm.github.io/wasm-pack/) and [pnpm](https://pnpm.io).

```sh
rustup target add wasm32-unknown-unknown
pnpm install
```

| Command              | Description                                                            |
| -------------------- | ---------------------------------------------------------------------- |
| `pnpm test`          | Builds the wasm module, then runs the Rust and TypeScript test suites. |
| `pnpm run build`     | Builds the wasm module, the bindings and the bundle into `dist/`.      |
| `pnpm run typecheck` | Runs `tsc --noEmit`.                                                   |
| `pnpm run fmt`       | Formats with Prettier.                                                 |

Two directories are generated and not checked in: `crates/comparer/pkg` (wasm-pack output) and `src/bindings` (ts-rs output). Both are produced by `pnpm test` and `pnpm run build`, so a fresh clone needs no extra steps.

## License

[MIT](LICENSE)

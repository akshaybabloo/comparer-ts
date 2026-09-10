# comparer-ts

A TypeScript library for generating diffs between strings, powered by WebAssembly.

The diffing itself is done in Rust by [similar](https://github.com/mitsuhiko/similar) and compiled to WebAssembly, so it stays fast on large inputs while the public API remains plain TypeScript.

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

### Types

`LineDiff`, `InlineLineDiff` and `Segment` are generated from the Rust structs by [ts-rs](https://github.com/Aleph-Alpha/ts-rs) and exported from the package root:

```ts
import type { InlineLineDiff, LineDiff, Segment } from "comparer-ts";
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

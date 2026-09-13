import { FolderComparer, Hasher, generate_diff, generate_inline_diff } from "../crates/comparer/pkg/comparer.js";
import type { FolderDiff } from "./bindings/FolderDiff";
import type { FsEntry } from "./bindings/FsEntry";
import type { HashJob } from "./bindings/HashJob";
import type { InlineLineDiff } from "./bindings/InlineLineDiff";
import type { LineDiff } from "./bindings/LineDiff";
import type { Side } from "./bindings/Side";

export type { FolderDiff, FsEntry, InlineLineDiff, LineDiff, Side };
export type { ChangeReason } from "./bindings/ChangeReason";
export type { ChangeStatus } from "./bindings/ChangeStatus";
export type { EntryKind } from "./bindings/EntryKind";
export type { FolderStats } from "./bindings/FolderStats";
export type { Segment } from "./bindings/Segment";
export type { TreeNode } from "./bindings/TreeNode";

/** Streams the content of one file from a listing, chunk by chunk. */
export type ReadFile = (side: Side, path: string, signal?: AbortSignal) => AsyncIterable<Uint8Array>;

export type HashProgress = {
  /** Files hashed so far, including ones that could not be read. */
  done: number;
  /** Files that need hashing in total. */
  total: number;
  /** Bytes hashed so far. */
  bytes: number;
};

export type CompareFoldersOptions = {
  /** Files read at once. Defaults to 8. */
  concurrency?: number;
  /** Stops reading and rejects with the signal's reason. */
  signal?: AbortSignal;
  /** Called after each file is hashed. */
  onProgress?: (progress: HashProgress) => void;
};

/**
 * Generates a diff between two strings.
 *
 * @param old_text The first string to compare.
 * @param new_text The second string to compare.
 * @returns The diff between the two strings.
 */
export function generateDiff(old_text: string, new_text: string): LineDiff[] {
  return JSON.parse(generate_diff(old_text, new_text));
}
/**
 * Generates an inline diff between two strings.
 *
 * @param old_text The first string to compare.
 * @param new_text The second string to compare.
 * @returns The inline diff between the two strings.
 */
export function generateInlineDiff(old_text: string, new_text: string): InlineLineDiff[] {
  return JSON.parse(generate_inline_diff(old_text, new_text));
}

/**
 * Compares two folder listings, hashing file content in WebAssembly.
 *
 * Only files present on both sides with the same size are read, through
 * `read`; any other content difference is already known from the listing. A
 * file that fails to read is reported as `unknown` rather than failing the
 * whole comparison.
 *
 * @param left Every entry of the original folder.
 * @param right Every entry of the changed folder.
 * @param read Streams a file's content, given the side and path of a listed entry.
 * @returns The merged tree, with a status for every entry.
 */
export async function compareFolders(
  left: FsEntry[],
  right: FsEntry[],
  read: ReadFile,
  { concurrency = 8, signal, onProgress }: CompareFoldersOptions = {},
): Promise<FolderDiff> {
  signal?.throwIfAborted();
  const comparer = new FolderComparer(JSON.stringify(left), JSON.stringify(right));

  try {
    const jobs: HashJob[] = JSON.parse(comparer.pending_hashes());
    const progress: HashProgress = { done: 0, total: jobs.length, bytes: 0 };
    let next = 0;
    let stopped = false;

    const worker = async () => {
      try {
        while (next < jobs.length && !stopped && !signal?.aborted) {
          const job = jobs[next++];
          try {
            const chunks = read(job.side, job.path, signal);
            const hash = await hashChunks(chunks, signal, (bytes) => (progress.bytes += bytes));
            comparer.set_hash(job.id, hash);
          } catch (error) {
            if (signal?.aborted) return;
            comparer.set_error(job.id, error instanceof Error ? error.message : String(error));
          }
          progress.done++;
          onProgress?.({ ...progress });
        }
      } catch (error) {
        stopped = true;
        throw error;
      }
    };

    // Settle every worker before rethrowing, so none is still using the
    // comparer when it is freed.
    const workers = Math.max(1, Math.min(concurrency, jobs.length));
    const results = await Promise.allSettled(Array.from({ length: workers }, worker));
    signal?.throwIfAborted();
    const failure = results.find((result) => result.status === "rejected");
    if (failure) throw failure.reason;
  } catch (error) {
    comparer.free();
    throw error;
  }

  return JSON.parse(comparer.finish());
}

async function hashChunks(
  chunks: AsyncIterable<Uint8Array>,
  signal: AbortSignal | undefined,
  onBytes: (bytes: number) => void,
): Promise<string> {
  const hasher = new Hasher();
  try {
    for await (const chunk of chunks) {
      signal?.throwIfAborted();
      hasher.update(chunk);
      onBytes(chunk.byteLength);
    }
  } catch (error) {
    hasher.free();
    throw error;
  }
  return hasher.finish();
}

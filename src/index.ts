import {
  FolderComparer,
  Hasher,
  compare_images,
  compare_images_rgba,
  generate_diff,
  generate_inline_diff,
  type ImageComparison,
} from "../crates/comparer/pkg/comparer.js";
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
export type { LineTag } from "./bindings/LineTag";
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

export type CompareImagesOptions = {
  /**
   * How much colour difference to tolerate, from 0 to 100. At 0, the default, any change
   * to a pixel counts; at 100 every change is tolerated.
   */
  tolerance?: number;
  /** Paint the diff image as raw RGBA pixels, in `diff_rgba`. */
  diffRgba?: boolean;
  /** Paint the diff image as PNG bytes, in `diff_png`. */
  diffPng?: boolean;
};

export type ImageDiff = {
  width: number;
  height: number;
  /** Pixels that differ by more than the tolerance. */
  different_pixels: number;
  total_pixels: number;
  /** `different_pixels` as a percentage of `total_pixels`, or 0 for an empty image. */
  percent: number;
  /** No pixel differs by more than the tolerance. */
  identical: boolean;
  /**
   * The diff image as RGBA, row by row: differing pixels red, the rest of the left image
   * faded to grey. Ready for `new ImageData(diff_rgba, width, height)`. `null` unless
   * `diffRgba` is set.
   */
  diff_rgba: Uint8ClampedArray<ArrayBuffer> | null;
  /** The same diff image encoded as PNG. `null` unless `diffPng` is set. */
  diff_png: Uint8Array<ArrayBuffer> | null;
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

/**
 * Compares two images pixel by pixel.
 *
 * The images are decoded in WebAssembly and may be in different formats, but must be the
 * same size.
 *
 * @param left The original image, as PNG, JPEG, WebP, GIF or BMP bytes.
 * @param right The changed image, in any of the same formats.
 * @returns How many pixels differ, and the diff image if asked for.
 */
export function compareImages(left: Uint8Array, right: Uint8Array, options: CompareImagesOptions = {}): ImageDiff {
  const { tolerance = 0, diffRgba = false, diffPng = false } = options;
  return toImageDiff(compare_images(left, right, tolerance, diffRgba, diffPng));
}

/**
 * Compares two images given as raw RGBA pixels, row by row, such as a canvas's
 * `ImageData.data`. Nothing is decoded, so this is faster than {@link compareImages}.
 *
 * @param left The original image's pixels.
 * @param right The changed image's pixels.
 * @param width The width of both images.
 * @param height The height of both images.
 * @returns How many pixels differ, and the diff image if asked for.
 */
export function compareImagesRgba(
  left: Uint8Array | Uint8ClampedArray,
  right: Uint8Array | Uint8ClampedArray,
  width: number,
  height: number,
  options: CompareImagesOptions = {},
): ImageDiff {
  const { tolerance = 0, diffRgba = false, diffPng = false } = options;
  return toImageDiff(compare_images_rgba(asBytes(left), asBytes(right), width, height, tolerance, diffRgba, diffPng));
}

/** Views clamped pixels as plain bytes, without copying them. */
function asBytes(pixels: Uint8Array | Uint8ClampedArray): Uint8Array {
  return pixels instanceof Uint8Array ? pixels : new Uint8Array(pixels.buffer, pixels.byteOffset, pixels.byteLength);
}

function toImageDiff(comparison: ImageComparison): ImageDiff {
  try {
    const { width, height, different_pixels } = comparison;
    const total_pixels = width * height;
    // Copied out of WebAssembly memory into buffers of their own.
    const rgba = comparison.take_diff_rgba();
    const png = comparison.take_diff_png();

    return {
      width,
      height,
      different_pixels,
      total_pixels,
      percent: total_pixels === 0 ? 0 : (different_pixels / total_pixels) * 100,
      identical: different_pixels === 0,
      diff_rgba: rgba ? new Uint8ClampedArray(rgba.buffer as ArrayBuffer, rgba.byteOffset, rgba.byteLength) : null,
      diff_png: png ? new Uint8Array(png.buffer as ArrayBuffer, png.byteOffset, png.byteLength) : null,
    };
  } finally {
    comparison.free();
  }
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

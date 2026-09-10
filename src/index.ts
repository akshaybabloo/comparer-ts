import { generate_diff, generate_inline_diff } from "../crates/comparer/pkg/comparer.js";
import type { LineDiff } from "./bindings/LineDiff";
import type { InlineLineDiff } from "./bindings/InlineLineDiff";

export type { LineDiff, InlineLineDiff };
export type { Segment } from "./bindings/Segment";

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

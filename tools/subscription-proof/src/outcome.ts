// Decides what a finished (or broken) generation stream may commit. Used by every provider stream.

export type StreamResult = {
  /** Provider reported terminal success (turn completed / response.completed). */
  completed: boolean;
  /** Final complete text, when the provider delivered one. */
  text: string | null;
  /** Text streamed so far (deltas), kept even when the stream breaks. */
  partialText: string;
  /** Tool items, server requests, or any other non-text activity observed. */
  unsafeEvents: string[];
  /** Provider status or error when not completed. */
  failure: string | null;
};

export type Settlement =
  | { outcome: "generated"; output: string }
  | { outcome: "failed"; error: string; partial: string | null };

/**
 * Only a completed, tool-free stream commits output. Anything else fails closed; whatever text
 * arrived is returned as `partial` for separate retention, never as the complete output.
 */
export function settle(r: StreamResult): Settlement {
  const retained = r.text ?? r.partialText;
  const partial = retained.length > 0 ? retained : null;
  if (r.unsafeEvents.length > 0) return { outcome: "failed", error: `unsafe events: ${r.unsafeEvents.join(",")}`, partial };
  if (r.completed && r.text !== null) return { outcome: "generated", output: r.text };
  return { outcome: "failed", error: r.failure ?? "no complete output", partial };
}

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ActiveMutationInfo } from "./types";

export const EXIT_REQUESTED_EVENT = "exit-requested";

/**
 * Window close / Cmd+Q is vetoed in Rust while a mutation runs; the backend
 * emits this event with the active task so the renderer can explain the risk.
 * In tests (and if the bridge is unavailable) subscribing is a no-op.
 */
export async function listenForExitRequest(
  handler: (info: ActiveMutationInfo) => void,
): Promise<UnlistenFn> {
  try {
    return await listen<ActiveMutationInfo>(EXIT_REQUESTED_EVENT, (event) => {
      handler(event.payload);
    });
  } catch {
    return () => {};
  }
}

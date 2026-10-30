//! The only native folder picker in the app. Kept behind a module seam so App
//! tests can stub the dialog plugin without mocking the Tauri IPC bridge.

import { open } from "@tauri-apps/plugin-dialog";

/** Returns the canonical folder chosen by the user, or null when cancelled. */
export async function pickProjectFolder(): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择项目目录",
  });
  return typeof selected === "string" ? selected : null;
}

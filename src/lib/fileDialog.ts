import { invoke } from "@tauri-apps/api/core";
import * as dialog from "@tauri-apps/plugin-dialog";

/**
 * The dialog plugin's file dialogs, used in their place: on Windows, WebView2 hides the
 * pointer while the user types, and a dialog opened right after (`rz` in the terminal) would
 * have none. The backend shows it while the dialog is open (`win::file_dialog`).
 */
async function withPointer<T>(show: () => Promise<T>): Promise<T> {
  await invoke("window_file_dialog", { open: true }).catch(console.error);
  try {
    return await show();
  } finally {
    void invoke("window_file_dialog", { open: false }).catch(console.error);
  }
}

export const open: typeof dialog.open = (options) => withPointer(() => dialog.open(options));
export const save: typeof dialog.save = (options) => withPointer(() => dialog.save(options));

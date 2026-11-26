import { invoke } from "@tauri-apps/api/core";

/**
 * Read the system clipboard as text ("" when empty or unreadable).
 *
 * Goes through the backend first: WebKit, the webview on macOS and Linux,
 * gates `navigator.clipboard.readText` behind a paste prompt or refuses it.
 */
export async function readClipboardText(): Promise<string> {
  try {
    const text = await invoke<string | null>("read_clipboard_text");
    if (text) return text;
  } catch {
    // Backend clipboard unavailable — try the web API.
  }
  try {
    return (await navigator.clipboard?.readText?.()) ?? "";
  } catch {
    return "";
  }
}

/**
 * Copy text to the clipboard through the backend, falling back to the web
 * clipboard API and then to `execCommand` (the only path on some WebKitGTK
 * builds).
 */
export function copyTextWithFallback(text: string): void {
  if (!text) return;
  const legacyFallback = () => {
    const textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.select();
    try {
      document.execCommand("copy");
    } catch (err) {
      console.error("Failed to copy to clipboard", err);
    } finally {
      document.body.removeChild(textarea);
    }
  };
  const webFallback = () => {
    if (typeof navigator.clipboard?.writeText === "function") {
      navigator.clipboard.writeText(text).catch(legacyFallback);
      return;
    }
    legacyFallback();
  };
  invoke("write_clipboard_text", { text }).catch(webFallback);
}

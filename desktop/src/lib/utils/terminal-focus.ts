/** True when keyboard focus is inside an xterm.js instance. */
export function isTerminalFocused(): boolean {
  const active = document.activeElement;
  if (!active) return false;
  return (
    active.classList.contains("xterm-helper-textarea") ||
    active.closest(".xterm") !== null
  );
}

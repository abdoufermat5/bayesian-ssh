import {
  Clock,
  HardDrive,
  KeyRound,
  Server,
  ShieldCheck,
  TerminalSquare,
  Users,
} from "lucide-svelte";
import type { AppTab } from "$lib/types";

/** Display name and icon of each numbered view; order comes from VIEW_ORDER in shortcuts.ts. */
export const VIEW_META: Record<
  Exclude<AppTab, "settings">,
  { label: string; icon: typeof Server }
> = {
  connections: { label: "Hosts", icon: Server },
  terminals: { label: "Terminals", icon: TerminalSquare },
  sftp: { label: "Files", icon: HardDrive },
  keys: { label: "Keys", icon: KeyRound },
  audit: { label: "Security check", icon: ShieldCheck },
  history: { label: "History", icon: Clock },
  teams: { label: "Teams", icon: Users },
};

/** Whether the settings turn this view off (Files can be disabled). */
export function viewEnabled(
  tab: AppTab,
  settings: { enable_sftp?: boolean },
): boolean {
  if (tab === "sftp") return settings.enable_sftp !== false;
  return true;
}

/**
 * Terminal tab groups: named, coloured sets of tabs kept next to each other
 * in the tab strip. A group remembers the hosts of its tabs, so a later
 * session to one of them (also after a restart) joins it again.
 */

/** Colours reuse the theme's terminal palette, so every theme gets matching shades. */
export const TAB_GROUP_COLORS = ["blue", "green", "yellow", "red", "magenta", "cyan", "grey"] as const;
export type TabGroupColor = (typeof TAB_GROUP_COLORS)[number];

export interface TabGroup {
  id: string;
  name: string;
  color: TabGroupColor;
  collapsed: boolean;
  /** Connection ids whose new sessions join this group. */
  hosts: string[];
}

export function tabGroupColorVar(color: TabGroupColor): string {
  return color === "grey" ? "var(--term-bright-black)" : `var(--term-${color})`;
}

export function tabGroupLabel(group: Pick<TabGroup, "name">): string {
  return group.name.trim() || "Unnamed group";
}

/** First colour no other group uses, cycling once all are taken. */
export function nextTabGroupColor(groups: readonly TabGroup[]): TabGroupColor {
  const used = new Set(groups.map((g) => g.color));
  return TAB_GROUP_COLORS.find((c) => !used.has(c)) ?? TAB_GROUP_COLORS[groups.length % TAB_GROUP_COLORS.length];
}

const STORAGE_KEY = "bayesian-ssh-tab-groups";

function isTabGroup(value: unknown): value is TabGroup {
  if (!value || typeof value !== "object") return false;
  const g = value as Record<string, unknown>;
  return (
    typeof g.id === "string" &&
    typeof g.name === "string" &&
    TAB_GROUP_COLORS.includes(g.color as TabGroupColor) &&
    typeof g.collapsed === "boolean" &&
    Array.isArray(g.hosts) &&
    g.hosts.every((h) => typeof h === "string")
  );
}

export function loadTabGroups(): TabGroup[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]");
    return Array.isArray(parsed) ? parsed.filter(isTabGroup) : [];
  } catch {
    return [];
  }
}

export function saveTabGroups(groups: readonly TabGroup[]): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(groups));
  } catch {
    // Storage full or unavailable: groups still work for this session.
  }
}

/**
 * Pull each group's tabs together at the place of its first tab, keeping
 * every other order. Returns the input when it is already contiguous.
 */
export function groupContiguous<T extends { groupId: string | null }>(tabs: T[]): T[] {
  const out: T[] = [];
  const placed = new Set<string>();
  for (const tab of tabs) {
    if (!tab.groupId) {
      out.push(tab);
    } else if (!placed.has(tab.groupId)) {
      placed.add(tab.groupId);
      out.push(...tabs.filter((t) => t.groupId === tab.groupId));
    }
  }
  return out.every((t, i) => t === tabs[i]) ? tabs : out;
}

/**
 * Group a tab gets when dropped between `before` and `after`: inside a group
 * it joins that group; at a group's edge it keeps its own group if that is
 * the neighbour's, and is otherwise ungrouped.
 */
export function groupAtDrop(
  own: string | null,
  before: { groupId: string | null } | undefined,
  after: { groupId: string | null } | undefined,
): string | null {
  if (before?.groupId && before.groupId === after?.groupId) return before.groupId;
  if (own && (before?.groupId === own || after?.groupId === own)) return own;
  return null;
}

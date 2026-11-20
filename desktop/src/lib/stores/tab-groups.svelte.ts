import {
  loadTabGroups,
  saveTabGroups,
  type TabGroup,
  type TabGroupColor,
} from "$lib/utils/tab-groups";

/** Saved in localStorage; replaced wholesale on change. */
let groups = $state.raw<TabGroup[]>(loadTabGroups());

/** Tab groups, open or remembered, in creation order. */
export function getTabGroups(): readonly TabGroup[] {
  return groups;
}

export function findGroup(groupId: string | null): TabGroup | undefined {
  return groupId ? groups.find((g) => g.id === groupId) : undefined;
}

/** Group a remembered host joins with new sessions, if any. */
export function groupForHost(connId: string): string | null {
  return groups.find((g) => g.hosts.includes(connId))?.id ?? null;
}

function commitGroups(next: TabGroup[]) {
  groups = next;
  saveTabGroups(next);
}

export function addGroup(group: TabGroup) {
  commitGroups([...groups, group]);
}

export function deleteGroup(groupId: string) {
  commitGroups(groups.filter((g) => g.id !== groupId));
}

function updateGroup(groupId: string, patch: Partial<Omit<TabGroup, "id">>) {
  commitGroups(groups.map((g) => (g.id === groupId ? { ...g, ...patch } : g)));
}

export function setGroupCollapsed(groupId: string, collapsed: boolean) {
  updateGroup(groupId, { collapsed });
}

export function expandTabGroup(groupId: string) {
  if (findGroup(groupId)?.collapsed) updateGroup(groupId, { collapsed: false });
}

/** New sessions to `connId` join `groupId` (none: no group). A host is in one group at most. */
export function rememberHost(connId: string, groupId: string | null) {
  commitGroups(
    groups.map((g) => {
      const has = g.hosts.includes(connId);
      if (g.id === groupId) return has ? g : { ...g, hosts: [...g.hosts, connId] };
      return has ? { ...g, hosts: g.hosts.filter((h) => h !== connId) } : g;
    }),
  );
}

/** Stored as typed (it is edited live); labels trim it. */
export function renameTabGroup(groupId: string, name: string) {
  updateGroup(groupId, { name });
}

export function setTabGroupColor(groupId: string, color: TabGroupColor) {
  updateGroup(groupId, { color });
}

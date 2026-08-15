/**
 * Connection list / search / selection domain store.
 *
 * Owns the connection registry, the search+filter state, list/grid view
 * preferences, connection statistics, clipboard/duplicate feedback flags,
 * and the Add/Edit connection modal form fields. The modal visibility flag
 * itself lives in `modals.svelte.ts`.
 */
import { invoke } from "@tauri-apps/api/core";
import type { Connection, ConnectionStats } from "$lib/types";
import { notify } from "$lib/stores/notifications.svelte";
import { getModalsState } from "$lib/stores/modals.svelte";

const modals = getModalsState();

let connections = $state<Connection[]>([]);
let searchQuery = $state("");
let selectedTag = $state<string | null>(null);
let selectedHostIndex = $state(0);
let viewMode = $state<"list" | "grid">("list");
let stats = $state<ConnectionStats | null>(null);
let copiedId = $state<string | null>(null);
let justDuplicatedId = $state<string | null>(null);

// Connection modal form state.
let isEditing = $state(false);
let modalConnectionId = $state("");
let modalName = $state("");
let modalHost = $state("");
let modalUser = $state("");
let modalPort = $state(22);
let modalUseKerberos = $state(false);
let modalBastion = $state("");
let modalBastionUser = $state("");
let modalKeyPath = $state("");
let modalTagsString = $state("");

let allTags = $derived.by(() => {
  const tagsSet = new Set<string>();
  connections.forEach((c) => c.tags.forEach((t) => tagsSet.add(t)));
  return Array.from(tagsSet).sort();
});

export async function loadConnections() {
  try {
    connections = await invoke("get_connections", {
      query: searchQuery,
      tagFilter: selectedTag,
    });
  } catch (e: unknown) {
    notify(String(e), "error");
  }
}

export async function reloadConnectionsAfterMutation() {
  try {
    const allConnections = await invoke<Connection[]>("get_connections", {
      query: "",
      tagFilter: null,
    });

    if (searchQuery.trim() || selectedTag) {
      const filtered = await invoke<Connection[]>("get_connections", {
        query: searchQuery,
        tagFilter: selectedTag,
      });

      if (filtered.length === 0 && allConnections.length > 0) {
        searchQuery = "";
        selectedTag = null;
        connections = allConnections;
        notify("Search cleared — the saved host no longer matches your filter", "info");
        return;
      }

      connections = filtered;
      return;
    }

    connections = allConnections;
  } catch (e: unknown) {
    notify(String(e), "error");
  }
}

export async function loadStats() {
  try {
    stats = await invoke("get_stats");
  } catch {
    // optional
  }
}

export async function duplicateConnection(conn: Connection) {
  try {
    const copyName = `${conn.name} (Copy)`;
    await invoke("add_connection", {
      name: copyName,
      host: conn.host,
      user: conn.user,
      port: conn.port,
      kerberos: conn.use_kerberos,
      bastion: conn.bastion || null,
      bastionUser: conn.bastion_user || null,
      keyPath: conn.key_path || null,
      tags: [...conn.tags],
    });

    await reloadConnectionsAfterMutation();
    await loadStats();
    try {
      await invoke("refresh_tray_menu");
    } catch (e) {
      console.error(e);
    }

    const newIdx = connections.findIndex((c) => c.name === copyName && c.host === conn.host);
    if (newIdx !== -1) {
      selectedHostIndex = newIdx;
      justDuplicatedId = connections[newIdx].id;
      setTimeout(() => {
        justDuplicatedId = null;
      }, 2000);
      openEditModal(connections[newIdx]);
    }

    notify("Connection duplicated — update values below", "info");
  } catch (e: unknown) {
    notify(`Failed to duplicate: ${e}`, "error");
  }
}

export async function copyToClipboard(text: string, id: string) {
  try {
    await navigator.clipboard.writeText(text);
    copiedId = id;
    setTimeout(() => {
      if (copiedId === id) copiedId = null;
    }, 1500);
    notify("SSH command copied to clipboard", "success");
  } catch {
    notify("Failed to copy", "error");
  }
}

export async function browseKey() {
  try {
    const selected = await invoke<string | null>("pick_key_file");
    if (selected) modalKeyPath = selected;
  } catch (e: unknown) {
    notify(String(e), "error");
  }
}

export function openAddModal() {
  isEditing = false;
  modalConnectionId = "";
  modalName = "";
  modalHost = "";
  modalUser = "";
  modalPort = 22;
  modalUseKerberos = false;
  modalBastion = "";
  modalBastionUser = "";
  modalKeyPath = "";
  modalTagsString = "";
  modals.showModal = true;
}

export function openEditModal(conn: Connection) {
  isEditing = true;
  modalConnectionId = conn.id;
  modalName = conn.name;
  modalHost = conn.host;
  modalUser = conn.user;
  modalPort = conn.port;
  modalUseKerberos = conn.use_kerberos;
  modalBastion = conn.bastion || "";
  modalBastionUser = conn.bastion_user || "";
  modalKeyPath = conn.key_path || "";
  modalTagsString = conn.tags.join(", ");
  modals.showModal = true;
}

export async function saveConnection() {
  if (!modalName.trim() || !modalHost.trim()) {
    notify("Name and Host are required.", "error");
    return;
  }

  const tags = modalTagsString
    .split(",")
    .map((t) => t.trim())
    .filter((t) => t.length > 0);

  const payload = {
    name: modalName.trim(),
    host: modalHost.trim(),
    user: modalUser.trim() || null,
    port: modalPort || null,
    kerberos: modalUseKerberos || null,
    bastion: modalBastion.trim() || null,
    bastionUser: modalBastionUser.trim() || null,
    keyPath: modalKeyPath.trim() || null,
    tags,
  };

  try {
    if (isEditing) {
      await invoke("edit_connection", {
        id: modalConnectionId,
        ...payload,
        user: modalUser,
        port: modalPort,
        kerberos: modalUseKerberos,
      });
      notify("Host updated successfully", "success");
    } else {
      await invoke("add_connection", payload);
      notify("Host added successfully", "success");
    }
    modals.showModal = false;
    await reloadConnectionsAfterMutation();
    await loadStats();
    try {
      await invoke("refresh_tray_menu");
    } catch (e) {
      console.error(e);
    }
  } catch (e: unknown) {
    notify(String(e), "error");
  }
}

export function deleteConnection(conn: Connection) {
  modals.promptDelete(conn.name, `${conn.user}@${conn.host}:${conn.port}`, async () => {
    await invoke("remove_connection", { idOrName: conn.id });
    notify(`'${conn.name}' removed`, "success");
    await loadConnections();
    await loadStats();
    try {
      await invoke("refresh_tray_menu");
    } catch (e) {
      console.error(e);
    }
  });
}

export function getConnectionsState() {
  return {
    get connections() {
      return connections;
    },
    set connections(value: Connection[]) {
      connections = value;
    },
    get searchQuery() {
      return searchQuery;
    },
    set searchQuery(value: string) {
      searchQuery = value;
    },
    get selectedTag() {
      return selectedTag;
    },
    set selectedTag(value: string | null) {
      selectedTag = value;
    },
    get selectedHostIndex() {
      return selectedHostIndex;
    },
    set selectedHostIndex(value: number) {
      selectedHostIndex = value;
    },
    get viewMode() {
      return viewMode;
    },
    set viewMode(value: "list" | "grid") {
      viewMode = value;
    },
    get stats() {
      return stats;
    },
    set stats(value: ConnectionStats | null) {
      stats = value;
    },
    get copiedId() {
      return copiedId;
    },
    set copiedId(value: string | null) {
      copiedId = value;
    },
    get justDuplicatedId() {
      return justDuplicatedId;
    },
    set justDuplicatedId(value: string | null) {
      justDuplicatedId = value;
    },
    get isEditing() {
      return isEditing;
    },
    set isEditing(value: boolean) {
      isEditing = value;
    },
    get modalConnectionId() {
      return modalConnectionId;
    },
    set modalConnectionId(value: string) {
      modalConnectionId = value;
    },
    get modalName() {
      return modalName;
    },
    set modalName(value: string) {
      modalName = value;
    },
    get modalHost() {
      return modalHost;
    },
    set modalHost(value: string) {
      modalHost = value;
    },
    get modalUser() {
      return modalUser;
    },
    set modalUser(value: string) {
      modalUser = value;
    },
    get modalPort() {
      return modalPort;
    },
    set modalPort(value: number) {
      modalPort = value;
    },
    get modalUseKerberos() {
      return modalUseKerberos;
    },
    set modalUseKerberos(value: boolean) {
      modalUseKerberos = value;
    },
    get modalBastion() {
      return modalBastion;
    },
    set modalBastion(value: string) {
      modalBastion = value;
    },
    get modalBastionUser() {
      return modalBastionUser;
    },
    set modalBastionUser(value: string) {
      modalBastionUser = value;
    },
    get modalKeyPath() {
      return modalKeyPath;
    },
    set modalKeyPath(value: string) {
      modalKeyPath = value;
    },
    get modalTagsString() {
      return modalTagsString;
    },
    set modalTagsString(value: string) {
      modalTagsString = value;
    },
    get allTags() {
      return allTags;
    },
    loadConnections,
    reloadConnectionsAfterMutation,
    loadStats,
    duplicateConnection,
    copyToClipboard,
    browseKey,
    openAddModal,
    openEditModal,
    saveConnection,
    deleteConnection,
  };
}

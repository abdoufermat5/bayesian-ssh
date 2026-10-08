<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { listen } from "@tauri-apps/api/event";

  import TitleBar from "$lib/components/TitleBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ConnectionsView from "$lib/components/ConnectionsView.svelte";
  import TerminalsView from "$lib/components/TerminalsView.svelte";
  import HistoryView from "$lib/components/HistoryView.svelte";
  import KeysView from "$lib/components/KeysView.svelte";
  import AuditView from "$lib/components/AuditView.svelte";
  import SettingsView from "$lib/components/SettingsView.svelte";
  import SFTPView from "$lib/components/SFTPView.svelte";
  import TunnelStudioView from "$lib/components/TunnelStudioView.svelte";
  import SnippetsModal from "$lib/components/modals/SnippetsModal.svelte";
  import ConnectionModal from "$lib/components/modals/ConnectionModal.svelte";
  import EnvModal from "$lib/components/modals/EnvModal.svelte";
  import AgentModal from "$lib/components/modals/AgentModal.svelte";
  import KerberosModal from "$lib/components/modals/KerberosModal.svelte";
  import DetachedSessionsModal from "$lib/components/modals/DetachedSessionsModal.svelte";
  import DeleteConfirm from "$lib/components/modals/DeleteConfirm.svelte";
  import OnboardingModal from "$lib/components/modals/OnboardingModal.svelte";
  import ShortcutsModal from "$lib/components/modals/ShortcutsModal.svelte";
  import AboutModal from "$lib/components/modals/AboutModal.svelte";
  import BatchExecModal from "$lib/components/modals/BatchExecModal.svelte";
  import QuitConfirmModal from "$lib/components/modals/QuitConfirmModal.svelte";
  import AppLoader from "$lib/components/AppLoader.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import Toast from "$lib/components/Toast.svelte";
  import { invoke } from "@tauri-apps/api/core";

  import { notify } from "$lib/stores/notifications.svelte";
  import {
    getTerminalState,
    initTerminalListeners,
    teardownTerminalListeners,
    setTerminalsVisible,
    popOutDetachedSession,
    focusPopoutSession,
    terminateDetachedSession,
    terminatePopoutSession,
  } from "$lib/stores/terminal.svelte";
  import {
    getKerberosState,
    openKerberosModal,
    closeKerberosModal,
    stopKerberosMonitoring,
  } from "$lib/stores/kerberos.svelte";
  import { appState } from "$lib/stores/appState.svelte";

  const terminalState = getTerminalState();
  const kerberosState = getKerberosState();

  $effect(() => {
    // Keep the selected index in range when the list shrinks, but do NOT
    // reset it on every reload — that breaks duplicate-then-edit flows.
    const list = appState.connections;
    if (list.length > 0 && appState.selectedHostIndex >= list.length) {
      appState.selectedHostIndex = 0;
    }
  });

  // Re-query only when the filter actually changes (not on mount — loadData
  // already fetched the list).
  let lastFilterKey: string | null = null;
  $effect(() => {
    const key = `${appState.searchQuery}\u0000${appState.selectedTag ?? ""}`;
    if (lastFilterKey !== null && key !== lastFilterKey) appState.searchConnections();
    lastFilterKey = key;
  });

  // Single place that reacts to the Terminals view becoming visible:
  // resumes output rendering, then fits + focuses the active terminal.
  $effect(() => {
    const visible = appState.activeTab === "terminals";
    setTerminalsVisible(visible);
    if (visible) requestAnimationFrame(() => appState.goToTerminals());
  });

  let unlistenConnect: (() => void) | undefined;
  let unlistenQuitConfirm: (() => void) | undefined;
  let unlistenCloseRequested: (() => void) | undefined;

  let showShortcutsModal = $state(false);
  let showAboutModal = $state(false);
  let showBatchExecModal = $state(false);
  let showQuitConfirmModal = $state(false);
  let showSnippetsModal = $state(false);
  let showCommandPalette = $state(false);

  function handleKeydownWithHelp(e: KeyboardEvent) {
    const isEditingInput =
      document.activeElement?.tagName === "INPUT" ||
      document.activeElement?.tagName === "TEXTAREA" ||
      document.activeElement?.getAttribute("contenteditable") === "true";

    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      showCommandPalette = !showCommandPalette;
      return;
    }

    if (
      (e.key === "?" && !isEditingInput) ||
      e.key === "F1" ||
      ((e.ctrlKey || e.metaKey) && e.key === "/")
    ) {
      e.preventDefault();
      showShortcutsModal = !showShortcutsModal;
      return;
    }

    appState.handleGlobalKeydown(e);
  }

  onMount(() => {
    (async () => {
      const needsOnboarding = await appState.checkOnboarding();
      if (!needsOnboarding) {
        await appState.loadData();
      }
      appState.isInitializing = false;
    })();
    window.addEventListener("keydown", handleKeydownWithHelp);

    initTerminalListeners(async () => {
      await appState.loadHistory();
      await appState.loadStats();
    });

    const appWindow = getCurrentWindow();
    void appWindow
      .onCloseRequested((event) => {
        event.preventDefault();
        const activeCount = terminalState.count + terminalState.externalSessionCount;
        if (activeCount > 0) {
          showQuitConfirmModal = true;
          void appWindow.unminimize();
          void appWindow.show();
          void appWindow.setFocus();
        } else {
          void appWindow.hide();
        }
      })
      .then((unsub) => {
        unlistenCloseRequested = unsub;
      });

    listen("prompt-quit-confirm", () => {
      showQuitConfirmModal = true;
      void appWindow.unminimize();
      void appWindow.show();
      void appWindow.setFocus();
    }).then((unsub) => {
      unlistenQuitConfirm = unsub;
    });

    listen("connect-host", async (event) => {
      const hostName = event.payload as string;
      const conn = appState.connections.find((c) => c.name === hostName);
      if (conn) {
        await appState.handleConnect(conn);
      }
    }).then((unsub) => {
      unlistenConnect = unsub;
    });

    function handleWindowResize() {
      if (window.innerWidth < 860 && !appState.sidebarCollapsed) {
        appState.sidebarCollapsed = true;
      }
    }
    window.addEventListener("resize", handleWindowResize);
    if (window.innerWidth < 860) {
      appState.sidebarCollapsed = true;
    }

    return () => {
      window.removeEventListener("keydown", handleKeydownWithHelp);
      window.removeEventListener("resize", handleWindowResize);
      teardownTerminalListeners();
      stopKerberosMonitoring();
      unlistenConnect?.();
      unlistenQuitConfirm?.();
      unlistenCloseRequested?.();
    };
  });
</script>

{#if appState.isInitializing}
  <AppLoader />
{:else if appState.showOnboarding}
  <TitleBar
    onOpenAbout={() => (showAboutModal = true)}
    onOpenShortcuts={() => (showShortcutsModal = true)}
  />
  <OnboardingModal
    defaultUser={appState.settings.default_user}
    defaultSshConfigPath={appState.workspace.ssh_config_path || ""}
    configRoot={appState.workspace.config_root}
    onBrowseSshConfig={appState.browseSshConfig}
    onComplete={appState.completeOnboarding}
  />
{:else}
<div class="app-shell">
  <TitleBar
    onOpenAbout={() => (showAboutModal = true)}
    onOpenShortcuts={() => (showShortcutsModal = true)}
    onOpenCommandPalette={() => (showCommandPalette = true)}
  />

  <div class="flex min-h-0 w-full flex-1 overflow-hidden">
    <Sidebar
      activeTab={appState.activeTab}
      onTabChange={appState.handleTabChange}
      environments={appState.environments}
      activeEnv={appState.activeEnv}
      onSwitchEnv={appState.switchEnv}
      onShowEnvModal={() => (appState.showEnvModal = true)}
      sidebarCollapsed={appState.sidebarCollapsed}
      onToggleSidebar={() => (appState.sidebarCollapsed = !appState.sidebarCollapsed)}
      terminalCount={terminalState.totalSessionCount}
      externalSessionCount={terminalState.externalSessionCount}
      agentActive={appState.agentActive}
      agentKeys={appState.agentKeys}
      onStartAgent={appState.triggerStartAgent}
      onShowAgentModal={() => (appState.showAgentModal = true)}
      kerberosHealth={appState.kerberosHealth}
      kerberosRemainingLabel={appState.kerberosRemainingLabel}
      onShowKerberosModal={openKerberosModal}
      onShowSessionManager={appState.openSessionManager}
      onShowSnippetsModal={() => (showSnippetsModal = true)}
      settings={appState.settings}
    />

    <main class="workspace">
      {#if appState.activeTab === "connections"}
        <ConnectionsView
          connections={appState.connections}
          bind:viewMode={appState.viewMode}
          selectedHostIndex={appState.selectedHostIndex}
          copiedId={appState.copiedId}
          justDuplicatedId={appState.justDuplicatedId}
          timezone={appState.settings.timezone}
          bind:searchQuery={appState.searchQuery}
          bind:selectedTag={appState.selectedTag}
          allTags={appState.allTags}
          onSelectHost={(i) => (appState.selectedHostIndex = i)}
          onConnect={appState.handleConnect}
          onEdit={appState.openEditModal}
          onDelete={appState.deleteConnection}
          onDuplicate={appState.duplicateConnection}
          onCopyCommand={appState.copyToClipboard}
          onAddHost={appState.openAddModal}
          onImportSshConfig={appState.importSshConfig}
          onOpenBatchExec={() => (showBatchExecModal = true)}
        />
      {:else if appState.activeTab === "history"}
        <HistoryView history={appState.history} timezone={appState.settings.timezone} />
      {:else if appState.activeTab === "keys"}
        <KeysView connections={appState.connections} />
      {:else if appState.activeTab === "audit"}
        <AuditView />
      {:else if appState.activeTab === "settings"}
        <SettingsView
          bind:settings={appState.settings}
          bind:workspace={appState.workspace}
          environments={appState.environments}
          onSave={appState.saveSettings}
          onThemeChange={appState.handleThemeChange}
          onSaveWorkspace={appState.saveWorkspaceConfig}
          onSwitchEnv={appState.switchEnv}
          onManageProfiles={() => (appState.showEnvModal = true)}
          onBrowseSshConfig={appState.browseSshConfig}
          onImportSshConfig={appState.importSshConfig}
        />
      {:else if appState.activeTab === "sftp" && appState.settings.enable_sftp !== false}
        <SFTPView connections={appState.connections} />
      {:else if appState.activeTab === "tunnels" && appState.settings.enable_tunneling !== false}
        <TunnelStudioView connections={appState.connections} />
      {/if}

      <!-- Terminals stay mounted while sessions exist so xterm buffers
           survive tab switches; hidden with display:none (no layout/paint). -->
      {#if appState.showTerminalsPanel}
        <div class="view" class:hidden={appState.activeTab !== "terminals"}>
          <TerminalsView
            connections={appState.connections}
            bind:searchQuery={appState.searchQuery}
            onCloseAll={appState.requestCloseAllSessions}
            onManageSessions={appState.openSessionManager}
            onConnect={appState.handleConnect}
          />
        </div>
      {/if}
    </main>
  </div>

  {#if appState.showModal}
    <ConnectionModal
      isEditing={appState.isEditing}
      bind:modalName={appState.modalName}
      bind:modalHost={appState.modalHost}
      bind:modalUser={appState.modalUser}
      bind:modalPort={appState.modalPort}
      bind:modalUseKerberos={appState.modalUseKerberos}
      bind:modalBastion={appState.modalBastion}
      bind:modalBastionUser={appState.modalBastionUser}
      bind:modalKeyPath={appState.modalKeyPath}
      bind:modalTagsString={appState.modalTagsString}
      onClose={() => (appState.showModal = false)}
      onSave={appState.saveConnection}
      onBrowseKey={appState.browseKey}
    />
  {/if}

  {#if appState.showEnvModal}
    <EnvModal
      environments={appState.environments}
      bind:newEnvName={appState.newEnvName}
      onClose={() => (appState.showEnvModal = false)}
      onCreate={appState.createEnv}
      onDelete={appState.deleteEnv}
    />
  {/if}

  {#if appState.showAgentModal}
    <AgentModal
      agentSocket={appState.agentSocket}
      agentKeys={appState.agentKeys}
      onClose={() => (appState.showAgentModal = false)}
      onAddKey={appState.selectAndAddKey}
    />
  {/if}

  {#if appState.showSessionManager}
    <DetachedSessionsModal
      detachedSessions={terminalState.detachedSessions}
      popoutSessions={terminalState.popoutSessions}
      onClose={() => (appState.showSessionManager = false)}
      onReattach={appState.handleSessionReattach}
      onPopOut={popOutDetachedSession}
      onDock={appState.handleSessionDock}
      onFocusPopout={focusPopoutSession}
      onTerminateDetached={terminateDetachedSession}
      onTerminatePopout={terminatePopoutSession}
      onTerminateAll={appState.handleTerminateAllSessions}
    />
  {/if}

  {#if kerberosState.showModal && !appState.showOnboarding}
    <KerberosModal
      status={kerberosState.status}
      remainingSeconds={kerberosState.liveRemainingSeconds}
      ticketLifetimeSeconds={kerberosState.ticketLifetimeSeconds}
      loading={appState.kerberosLoading}
      error={appState.kerberosError}
      onClose={() => {
        appState.kerberosError = null;
        closeKerberosModal();
      }}
      onRenew={appState.handleKerberosRenew}
      onAcquire={appState.handleKerberosAcquire}
    />
  {/if}

  {#if appState.showDeleteConfirm && appState.deleteTarget}
    <DeleteConfirm
      title={appState.deleteTarget.title}
      confirmLabel={appState.deleteTarget.confirmLabel}
      warning={appState.deleteTarget.warning}
      label={appState.deleteTarget.label}
      subtitle={appState.deleteTarget.subtitle}
      onCancel={() => {
        appState.showDeleteConfirm = false;
        appState.deleteTarget = null;
      }}
      onConfirm={appState.confirmDelete}
    />
  {/if}

  <Toast />
</div>
{/if}

<ShortcutsModal show={showShortcutsModal} onClose={() => (showShortcutsModal = false)} />

<AboutModal
  show={showAboutModal}
  workspace={appState.workspace}
  activeEnv={appState.activeEnv}
  onClose={() => (showAboutModal = false)}
/>

<BatchExecModal
  show={showBatchExecModal}
  connections={appState.connections}
  onClose={() => (showBatchExecModal = false)}
/>

{#if showQuitConfirmModal}
  <QuitConfirmModal
    activeCount={terminalState.count + terminalState.externalSessionCount}
    onCancel={() => (showQuitConfirmModal = false)}
    onMinimize={async () => {
      showQuitConfirmModal = false;
      const appWindow = getCurrentWindow();
      await appWindow.hide();
    }}
    onQuit={async () => {
      showQuitConfirmModal = false;
      await invoke("force_quit_app");
    }}
  />
{/if}

<SnippetsModal
  show={showSnippetsModal}
  onClose={() => (showSnippetsModal = false)}
  onRunSnippet={(cmd) => {
    const tab = terminalState.tabs.find((t) => t.id === terminalState.activeTabId);
    if (!tab?.term) {
      // No active terminal — fall back to copying the command.
      notify("No active terminal — command copied to clipboard", "info");
      navigator.clipboard.writeText(cmd).catch(() => {});
      return;
    }
    const send = () => {
      // Resolve at send time: the tab may have closed while the confirm
      // dialog was open, leaving a disposed xterm behind.
      const term = terminalState.tabs.find((t) => t.id === tab.id)?.term;
      if (!term) {
        notify("Terminal closed — snippet not sent", "error");
        return;
      }
      // Bracketed paste would swallow a trailing "\r" inside the paste
      // markers, so Enter must be sent as a separate keystroke to execute.
      term.paste(cmd);
      term.input("\r");
    };
    if (appState.settings.confirm_snippet_execution !== false) {
      appState.promptDelete(
        "Send to active terminal",
        cmd,
        async () => send(),
        {
          title: "Execute command snippet",
          confirmLabel: "Execute",
          warning: "The command will be sent to the currently active SSH session.",
        },
      );
    } else {
      send();
    }
  }}
/>

<CommandPalette
  open={showCommandPalette}
  connections={appState.connections}
  activeTab={appState.activeTab}
  settings={appState.settings}
  onClose={() => (showCommandPalette = false)}
  onSelectTab={appState.handleTabChange}
  onConnectHost={appState.handleConnect}
  onOpenAddModal={appState.openAddModal}
  onOpenBatchExec={() => (showBatchExecModal = true)}
  onPingAll={() => {
    invoke<Array<{ success: boolean }>>("ping_all_connections")
      .then((results) => {
        const reachable = results.filter((r) => r.success).length;
        notify(`Ping completed: ${reachable}/${results.length} reachable hosts`, "success");
      })
      .catch((e) => notify(`Ping failed: ${e}`, "error"));
  }}
  onFixPermissions={async () => {
    try {
      const fixed = await invoke("fix_security_permissions");
      notify(`Repaired ${fixed} key permissions`, "success");
    } catch (e) {
      notify(`Fix failed: ${e}`, "error");
    }
  }}
  onOpenKeys={() => appState.handleTabChange("keys")}
  onOpenSessionManager={appState.openSessionManager}
  onSelectTheme={appState.handleThemeChange}
/>

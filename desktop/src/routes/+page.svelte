<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { listen } from "@tauri-apps/api/event";
  import { Plus, Search, List, LayoutGrid, OctagonX, Layers, TerminalSquare, X } from "lucide-svelte";

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
    popOutDetachedSession,
    focusPopoutSession,
    terminateDetachedSession,
    terminatePopoutSession,
  } from "$lib/stores/terminal.svelte";
  import { getWindowState, initWindowState } from "$lib/stores/window.svelte";
  import {
    getKerberosState,
    openKerberosModal,
    closeKerberosModal,
    startKerberosMonitoring,
    stopKerberosMonitoring,
  } from "$lib/stores/kerberos.svelte";
  import { appState } from "$lib/stores/appState.svelte";

  const terminalState = getTerminalState();
  const windowState = getWindowState();
  const kerberosState = getKerberosState();

  $effect(() => {
    // Keep the selected index in range when the list shrinks, but do NOT
    // reset it on every reload — that breaks duplicate-then-edit flows.
    const list = appState.connections;
    if (list.length > 0 && appState.selectedHostIndex >= list.length) {
      appState.selectedHostIndex = 0;
    }
  });

  $effect(() => {
    appState.searchQuery;
    appState.selectedTag;
    appState.searchConnections();
  });

  $effect(() => {
    if (appState.activeTab === "terminals") {
      requestAnimationFrame(() => appState.goToTerminals());
    }
  });

  let unlistenConnect: (() => void) | undefined;
  let unlistenQuitConfirm: (() => void) | undefined;

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

    if (appState.settings.monitor_kerberos && !appState.showOnboarding) {
      startKerberosMonitoring({
        warnMinutes: appState.settings.kerberos_warn_minutes,
        onWarning: (message) => {
          if (appState.showOnboarding) return;
          notify(message, "info");
          // Also send system notification
          import("@tauri-apps/api/core").then(({ invoke }) => {
            void invoke("send_desktop_notification", {
              title: "Kerberos Ticket Warning",
              body: message
            });
          });
        },
      });
    }

    let teardownWindow = () => {};
    initWindowState().then((teardown) => {
      teardownWindow = teardown;
    });

    const appWindow = getCurrentWindow();
    void appWindow.onCloseRequested((event) => {
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
      teardownWindow();
      unlistenConnect?.();
      unlistenQuitConfirm?.();
    };
  });
</script>

{#if appState.isInitializing}
  <AppLoader />
{:else if appState.showOnboarding}
  <TitleBar
    activeEnv={appState.activeEnv}
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
<div
  class="app-deck flex flex-col flex-1 w-full h-[100dvh] min-h-0 overflow-hidden"
  class:is-fullscreen={windowState.isFullscreen}
>
  <TitleBar
    activeEnv={appState.activeEnv}
    onOpenAbout={() => (showAboutModal = true)}
    onOpenShortcuts={() => (showShortcutsModal = true)}
    onOpenCommandPalette={() => (showCommandPalette = true)}
  />

  <div class="app-layer flex flex-1 min-h-0 w-full overflow-hidden">
    <Sidebar
      activeTab={appState.activeTab}
      onTabChange={appState.handleTabChange}
      environments={appState.environments}
      activeEnv={appState.activeEnv}
      onSwitchEnv={appState.switchEnv}
      onShowEnvModal={() => (appState.showEnvModal = true)}
      stats={appState.stats}
      sidebarCollapsed={appState.sidebarCollapsed}
      onToggleSidebar={() => (appState.sidebarCollapsed = !appState.sidebarCollapsed)}
      terminalCount={terminalState.totalSessionCount}
      tabCount={terminalState.count}
      externalSessionCount={terminalState.externalSessionCount}
      allTags={appState.allTags}
      selectedTag={appState.selectedTag}
      onTagSelect={(tag) => {
        appState.selectedTag = tag;
      }}
      agentActive={appState.agentActive}
      agentKeys={appState.agentKeys}
      onStartAgent={appState.triggerStartAgent}
      onShowAgentModal={() => (appState.showAgentModal = true)}
      kerberosHealth={appState.kerberosHealth}
      kerberosRemainingLabel={appState.kerberosRemainingLabel}
      kerberosPrincipal={kerberosState.status.principal}
      kerberosDefaultRealm={kerberosState.status.default_realm}
      onShowKerberosModal={openKerberosModal}
      onShowSessionManager={appState.openSessionManager}
      onGoToTerminals={appState.goToTerminals}
      onSearchMostUsed={(name) => (appState.searchQuery = name)}
      onShowSnippetsModal={() => (showSnippetsModal = true)}
      settings={appState.settings}
    />

    <main class="flex-1 flex flex-col min-w-0 min-h-0 overflow-hidden">
      <!-- Topbar -->
      <!-- Executive Topbar -->
      <header class="workspace-topbar">
        <!-- Left: Context / View Title -->
        <div class="flex items-center gap-2.5 min-w-0">
          <h1 class="text-sm font-semibold text-primary tracking-tight capitalize m-0 truncate">
            {appState.activeTab === "connections" ? "Hosts" : appState.activeTab}
          </h1>
          {#if appState.activeTab === "terminals" && terminalState.totalSessionCount > 0}
            <span class="hidden sm:inline px-2 py-0.5 rounded-full text-2xs font-mono bg-white/10 text-secondary whitespace-nowrap">
              {terminalState.count} active · {terminalState.externalSessionCount} away
            </span>
          {/if}
        </div>

        <!-- Center: Spotlight Command Palette Trigger -->
        <button
          type="button"
          class="command-trigger w-36 sm:w-56 md:w-72 max-w-sm"
          onclick={() => (showCommandPalette = true)}
          title="Search hosts or commands (⌘K)"
        >
          <div class="flex items-center gap-2 min-w-0">
            <Search size={13} class="text-muted shrink-0" />
            <span class="text-xs text-muted truncate">Search hosts, commands...</span>
          </div>
          <span class="kbd text-[10px] shrink-0">⌘K</span>
        </button>

        <!-- Right: Actions -->
        <div class="flex items-center gap-2 shrink-0">
          {#if appState.activeTab !== "terminals" && terminalState.totalSessionCount > 0}
            <button
              type="button"
              class="toolbar-btn h-8"
              onclick={appState.goToTerminals}
              title="Switch to active terminal sessions"
            >
              <TerminalSquare size={13} />
              <span class="hidden sm:inline">Terminals ({terminalState.totalSessionCount})</span>
              <span class="sm:hidden">({terminalState.totalSessionCount})</span>
            </button>
          {/if}

          <button
            type="button"
            class="btn btn-primary h-8"
            onclick={appState.openAddModal}
          >
            <Plus size={13} />
            <span class="hidden sm:inline">New Server</span>
            <span class="sm:hidden">New</span>
          </button>
        </div>
      </header>

      <!-- Main Body View Panels -->
      <div class="flex-1 min-h-0 relative overflow-hidden bg-surface/90">
        {#if appState.activeTab === "connections"}
          <div class="absolute inset-0 flex flex-col min-h-0 overflow-hidden transition-all duration-200 {appState.activeTab === 'connections' ? 'opacity-100 visible pointer-events-auto z-10' : 'opacity-0 invisible pointer-events-none z-0'}">
            <ConnectionsView
              connections={appState.connections}
              viewMode={appState.viewMode}
              selectedHostIndex={appState.selectedHostIndex}
              copiedId={appState.copiedId}
              justDuplicatedId={appState.justDuplicatedId}
              timezone={appState.settings.timezone}
              bind:searchQuery={appState.searchQuery}
              bind:selectedTag={appState.selectedTag}
              onSelectHost={(i) => (appState.selectedHostIndex = i)}
              onConnect={appState.handleConnect}
              onEdit={appState.openEditModal}
              onDelete={appState.deleteConnection}
              onDuplicate={appState.duplicateConnection}
              onCopyCommand={appState.copyToClipboard}
              onRefresh={appState.loadConnections}
              onAddHost={appState.openAddModal}
              onOpenBatchExec={() => (showBatchExecModal = true)}
            />
          </div>
        {/if}

        {#if appState.activeTab === "history"}
          <div class="absolute inset-0 flex flex-col min-h-0 overflow-hidden transition-all duration-200 {appState.activeTab === 'history' ? 'opacity-100 visible pointer-events-auto z-10' : 'opacity-0 invisible pointer-events-none z-0'}">
            <HistoryView history={appState.history} timezone={appState.settings.timezone} />
          </div>
        {/if}

        {#if appState.activeTab === "keys"}
          <div class="absolute inset-0 flex flex-col min-h-0 overflow-hidden transition-all duration-200 {appState.activeTab === 'keys' ? 'opacity-100 visible pointer-events-auto z-10' : 'opacity-0 invisible pointer-events-none z-0'}">
            <KeysView connections={appState.connections} />
          </div>
        {/if}

        {#if appState.activeTab === "audit"}
          <div class="absolute inset-0 flex flex-col min-h-0 overflow-hidden transition-all duration-200 {appState.activeTab === 'audit' ? 'opacity-100 visible pointer-events-auto z-10' : 'opacity-0 invisible pointer-events-none z-0'}">
            <AuditView />
          </div>
        {/if}

        {#if appState.activeTab === "settings"}
          <div class="absolute inset-0 flex flex-col min-h-0 overflow-hidden transition-all duration-200 {appState.activeTab === 'settings' ? 'opacity-100 visible pointer-events-auto z-10' : 'opacity-0 invisible pointer-events-none z-0'}">
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
          </div>
        {/if}

        {#if appState.activeTab === "sftp" && appState.settings.enable_sftp !== false}
          <div class="absolute inset-0 flex flex-col min-h-0 overflow-hidden transition-all duration-200 {appState.activeTab === 'sftp' ? 'opacity-100 visible pointer-events-auto z-10' : 'opacity-0 invisible pointer-events-none z-0'}">
            <SFTPView connections={appState.connections} />
          </div>
        {/if}

        {#if appState.activeTab === "tunnels" && appState.settings.enable_tunneling !== false}
          <div class="absolute inset-0 flex flex-col min-h-0 overflow-hidden transition-all duration-200 {appState.activeTab === 'tunnels' ? 'opacity-100 visible pointer-events-auto z-10' : 'opacity-0 invisible pointer-events-none z-0'}">
            <TunnelStudioView connections={appState.connections} />
          </div>
        {/if}

        {#if appState.showTerminalsPanel}
          <div class="absolute inset-0 flex flex-col min-h-0 overflow-hidden transition-all duration-200 {appState.activeTab === 'terminals' ? 'opacity-100 visible pointer-events-auto z-10' : 'opacity-0 invisible pointer-events-none z-0'}">
            <TerminalsView
              connections={appState.connections}
              bind:searchQuery={appState.searchQuery}
              onCloseAll={appState.requestCloseAllSessions}
              onManageSessions={appState.openSessionManager}
            />
          </div>
        {/if}
      </div>
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
      tab.term?.paste(`${cmd}\r`);
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
    invoke("ping_all_connections").catch(() => {});
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

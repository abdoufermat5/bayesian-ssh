<script lang="ts">
  import { onMount, type Component } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";

  import TitleBar from "$lib/components/TitleBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ConnectionsView from "$lib/components/ConnectionsView.svelte";
  import AttentionBanner from "$lib/components/AttentionBanner.svelte";
  import ApprovalDialog from "$lib/components/modals/ApprovalDialog.svelte";
  import AppLoader from "$lib/components/AppLoader.svelte";
  import Toast from "$lib/components/Toast.svelte";
  import { startAi, stopAiListeners } from "$lib/stores/ai.svelte";

  import { notify } from "$lib/stores/notifications.svelte";
  import {
    getTerminalState,
    acceptPendingChange,
    dismissPendingChange,
    initTerminalListeners,
    teardownTerminalListeners,
    setTerminalsVisible,
    popOutDetachedSession,
    focusPopoutSession,
    terminateDetachedSession,
    terminatePopoutSession,
  } from "$lib/stores/terminal.svelte";
  import { isTerminalPaletteKey } from "$lib/shortcuts";
  import {
    getKerberosState,
    openKerberosModal,
    closeKerberosModal,
    stopKerberosMonitoring,
  } from "$lib/stores/kerberos.svelte";
  import { stopTeamsAttentionPoller } from "$lib/stores/teamsAttention.svelte";
  import { appState } from "$lib/stores/appState.svelte";

  // Cached lazy loaders for secondary views and modals so they stay off the initial
  // chunk and never flicker/reload when revisited.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  type AnyComponent = Component<any>;

  const lazyLoaders = {
    terminals: () => import("$lib/components/TerminalsView.svelte"),
    history: () => import("$lib/components/HistoryView.svelte"),
    keys: () => import("$lib/components/KeysView.svelte"),
    audit: () => import("$lib/components/AuditView.svelte"),
    teams: () => import("$lib/components/TeamsView.svelte"),
    ai: () => import("$lib/components/AiView.svelte"),
    settings: () => import("$lib/components/SettingsView.svelte"),
    sftp: () => import("$lib/components/SFTPView.svelte"),
    onboarding: () => import("$lib/components/modals/OnboardingModal.svelte"),
    connectionModal: () => import("$lib/components/modals/ConnectionModal.svelte"),
    connectionChangeModal: () => import("$lib/components/modals/ConnectionChangeModal.svelte"),
    envModal: () => import("$lib/components/modals/EnvModal.svelte"),
    agentModal: () => import("$lib/components/modals/AgentModal.svelte"),
    detachedSessionsModal: () => import("$lib/components/modals/DetachedSessionsModal.svelte"),
    kerberosModal: () => import("$lib/components/modals/KerberosModal.svelte"),
    deleteConfirm: () => import("$lib/components/modals/DeleteConfirm.svelte"),
    shortcutsModal: () => import("$lib/components/modals/ShortcutsModal.svelte"),
    aboutModal: () => import("$lib/components/modals/AboutModal.svelte"),
    batchExecModal: () => import("$lib/components/modals/BatchExecModal.svelte"),
    uploadModal: () => import("$lib/components/modals/UploadModal.svelte"),
    quitConfirmModal: () => import("$lib/components/modals/QuitConfirmModal.svelte"),
    commandPalette: () => import("$lib/components/CommandPalette.svelte"),
  } as const;

  type LazyKey = keyof typeof lazyLoaders;

  let loaded = $state.raw<Partial<Record<LazyKey, AnyComponent>>>({});
  const inFlight = new Set<LazyKey>();

  function ensureLoaded(key: LazyKey) {
    if (loaded[key] || inFlight.has(key)) return;
    inFlight.add(key);
    void lazyLoaders[key]()
      .then((mod) => {
        loaded = { ...loaded, [key]: mod.default as AnyComponent };
      })
      .finally(() => {
        inFlight.delete(key);
      });
  }

  const terminalState = getTerminalState();
  const pendingPrompt = $derived(terminalState.pendingChange);
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

  // Single place that reacts to the terminal surface becoming visible:
  // resumes output rendering, then fits + focuses the active terminal. The
  // Console's home (host list) keeps the panes mounted but not rendered.
  let hadTabs = terminalState.tabs.length > 0;
  $effect(() => {
    const active = appState.activeTab;
    const count = terminalState.tabs.length;
    const visible = active === "terminals" && count > 0;
    setTerminalsVisible(visible);
    if (visible) {
      requestAnimationFrame(() => appState.goToTerminals());
    } else if (active === "terminals" && count === 0 && hadTabs) {
      // The last session went away while the terminal surface was showing;
      // the Console falls back to its home (host list).
      appState.activeTab = "connections";
    }
    hadTabs = count > 0;
  });

  let unlistenConnect: (() => void) | undefined;
  let unlistenQuitConfirm: (() => void) | undefined;
  let unlistenCloseRequested: (() => void) | undefined;

  const SIDEBAR_AUTO_COLLAPSE_PX = 1100;

  let showShortcutsModal = $state(false);
  let showAboutModal = $state(false);
  let showBatchExecModal = $state(false);
  let showUploadModal = $state(false);
  let uploadConnectionIds = $state<string[] | undefined>(undefined);
  let uploadRemoteDir = $state<string | null>(null);
  let showQuitConfirmModal = $state(false);
  let showCommandPalette = $state(false);

  $effect(() => {
    const tab = appState.activeTab;
    if (tab !== "connections" && tab !== "terminals") ensureLoaded(tab);
    if (appState.showTerminalsPanel) ensureLoaded("terminals");
    if (appState.showOnboarding) ensureLoaded("onboarding");
    if (appState.showModal) ensureLoaded("connectionModal");
    if (pendingPrompt) ensureLoaded("connectionChangeModal");
    if (appState.showEnvModal) ensureLoaded("envModal");
    if (appState.showAgentModal) ensureLoaded("agentModal");
    if (appState.showSessionManager) ensureLoaded("detachedSessionsModal");
    if (kerberosState.showModal && !appState.showOnboarding) ensureLoaded("kerberosModal");
    if (appState.showDeleteConfirm && appState.deleteTarget) ensureLoaded("deleteConfirm");
    if (showShortcutsModal) ensureLoaded("shortcutsModal");
    if (showAboutModal) ensureLoaded("aboutModal");
    if (showBatchExecModal) ensureLoaded("batchExecModal");
    if (showUploadModal) ensureLoaded("uploadModal");
    if (showQuitConfirmModal) ensureLoaded("quitConfirmModal");
    if (showCommandPalette) ensureLoaded("commandPalette");
  });

  /** Open the upload modal, optionally preselecting a host and destination. */
  function openUploadModal(connectionIds?: string[], remoteDir?: string | null) {
    uploadConnectionIds = connectionIds;
    uploadRemoteDir = remoteDir ?? null;
    showUploadModal = true;
  }

  function handleKeydownWithHelp(e: KeyboardEvent) {
    const isEditingInput =
      document.activeElement?.tagName === "INPUT" ||
      document.activeElement?.tagName === "TEXTAREA" ||
      document.activeElement?.getAttribute("contenteditable") === "true";

    // Plain Ctrl+K never reaches this handler from a focused terminal (xterm
    // consumes it for the shell), so terminals use Ctrl+Shift+P (Cmd+K on macOS).
    if (((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") || isTerminalPaletteKey(e)) {
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
      const [needsOnboarding] = await Promise.all([
        appState.checkOnboarding(),
        appState.loadCoreData(),
      ]);
      appState.isInitializing = false;
      if (!needsOnboarding) {
        appState.loadBackgroundData();
      }
    })();
    window.addEventListener("keydown", handleKeydownWithHelp);

    initTerminalListeners(async () => {
      await Promise.all([appState.loadHistory(), appState.loadStats()]);
    });

    void startAi();

    let disposed = false;

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
        if (disposed) unsub();
        else unlistenCloseRequested = unsub;
      })
      .catch((err) => {
        console.error("Failed to register close handler:", err);
      });

    listen("prompt-quit-confirm", () => {
      showQuitConfirmModal = true;
      void appWindow.unminimize();
      void appWindow.show();
      void appWindow.setFocus();
    })
      .then((unsub) => {
        if (disposed) unsub();
        else unlistenQuitConfirm = unsub;
      })
      .catch((err) => {
        console.error("Failed to listen for prompt-quit-confirm:", err);
      });

    listen("connect-host", async (event) => {
      const hostName = event.payload as string;
      const conn = appState.allConnections.find((c) => c.name === hostName);
      if (conn) {
        await appState.handleConnect(conn);
      }
    })
      .then((unsub) => {
        if (disposed) unsub();
        else unlistenConnect = unsub;
      })
      .catch((err) => {
        console.error("Failed to listen for connect-host:", err);
      });

    // Below 1100 px the sidebar collapses to icons by itself; the manual toggle
    // still works. It re-expands on widening only if we were the one to collapse it.
    let autoCollapsed = false;
    let wasNarrow = false;
    function handleWindowResize() {
      const narrow = window.innerWidth < SIDEBAR_AUTO_COLLAPSE_PX;
      if (narrow === wasNarrow) return;
      wasNarrow = narrow;
      if (narrow && !appState.sidebarCollapsed) {
        appState.sidebarCollapsed = true;
        autoCollapsed = true;
      } else if (!narrow && autoCollapsed) {
        appState.sidebarCollapsed = false;
        autoCollapsed = false;
      }
    }
    window.addEventListener("resize", handleWindowResize);
    handleWindowResize();

    return () => {
      disposed = true;
      window.removeEventListener("keydown", handleKeydownWithHelp);
      window.removeEventListener("resize", handleWindowResize);
      teardownTerminalListeners();
      stopAiListeners();
      stopKerberosMonitoring();
      stopTeamsAttentionPoller();
      unlistenConnect?.();
      unlistenQuitConfirm?.();
      unlistenCloseRequested?.();
    };
  });
</script>

<!-- The Console's home surface: the host list. Rendered on its own when no
     session exists, or inside TerminalsView as the pinned "Hosts" tab. -->
{#snippet connectionsView(compact = false)}
  <ConnectionsView
    {compact}
    connections={appState.connections}
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
    onOpenUpload={() => openUploadModal([])}
  />
{/snippet}

{#snippet terminalHome()}
  {@render connectionsView(true)}
{/snippet}

{#if appState.isInitializing}
  <AppLoader />
{:else if appState.showOnboarding}
  <TitleBar
    onOpenAbout={() => (showAboutModal = true)}
    onOpenShortcuts={() => (showShortcutsModal = true)}
  />
  {#if loaded.onboarding}
    {@const OnboardingModal = loaded.onboarding}
    <OnboardingModal
      defaultUser={appState.settings.default_user}
      defaultSshConfigPath={appState.workspace.ssh_config_path || ""}
      configRoot={appState.workspace.config_root}
      onBrowseSshConfig={appState.browseSshConfig}
      onComplete={appState.completeOnboarding}
    />
  {/if}
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
      kerberosInUse={appState.kerberosInUse}
      kerberosRemainingLabel={appState.kerberosRemainingLabel}
      onShowKerberosModal={openKerberosModal}
      onShowSessionManager={appState.openSessionManager}
      settings={appState.settings}
    />

    <main class="workspace">
      <AttentionBanner onTabChange={appState.handleTabChange} />
      {#if appState.activeTab === "history"}
        {#if loaded.history}
          {@const HistoryView = loaded.history}
          <HistoryView history={appState.history} timezone={appState.settings.timezone} />
        {/if}
      {:else if appState.activeTab === "keys"}
        {#if loaded.keys}
          {@const KeysView = loaded.keys}
          <KeysView connections={appState.allConnections} />
        {/if}
      {:else if appState.activeTab === "audit"}
        {#if loaded.audit}
          {@const AuditView = loaded.audit}
          <AuditView />
        {/if}
      {:else if appState.activeTab === "teams"}
        {#if loaded.teams}
          {@const TeamsView = loaded.teams}
          <TeamsView connections={appState.allConnections} onConnectionsChanged={() => void appState.loadConnections()} />
        {/if}
      {:else if appState.activeTab === "ai"}
        {#if loaded.ai}
          {@const AiView = loaded.ai}
          <AiView timezone={appState.settings.timezone} />
        {/if}
      {:else if appState.activeTab === "settings"}
        {#if loaded.settings}
          {@const SettingsView = loaded.settings}
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
        {/if}
      {:else if appState.activeTab === "sftp" && appState.settings.enable_sftp !== false}
        {#if loaded.sftp}
          {@const SFTPView = loaded.sftp}
          <SFTPView
            connections={appState.allConnections}
            onOpenUpload={(connectionId: string | null, remoteDir: string) =>
              openUploadModal(connectionId ? [connectionId] : [], remoteDir)}
          />
        {/if}
      {/if}

      <!-- Terminals stay mounted while sessions exist so xterm buffers
           survive view switches; hidden with display:none only when the
           Console is not the active workspace. With sessions open the Console
           is TerminalsView, whose first strip tab is the host list. -->
      {#if appState.showTerminalsPanel}
        <div
          class="view"
          class:hidden={appState.activeTab !== "connections" && appState.activeTab !== "terminals"}
        >
          {#if loaded.terminals}
            {@const TerminalsView = loaded.terminals}
            <TerminalsView
              connections={appState.allConnections}
              homeActive={appState.activeTab === "connections" || terminalState.tabs.length === 0}
              onGoHome={() => appState.handleTabChange("connections")}
              onCloseAll={appState.requestCloseAllSessions}
              onManageSessions={appState.openSessionManager}
              onConnect={(conn: import("$lib/types").Connection, options?: { split?: import("$lib/utils/split-layout").SplitSide }) =>
                appState.handleConnect(conn, { newTab: true, ...options })}
              home={terminalHome}
            />
          {/if}
        </div>
      {:else if appState.activeTab === "connections"}
        {@render connectionsView(false)}
      {/if}
    </main>
  </div>

  {#if appState.showModal && loaded.connectionModal}
    {@const ConnectionModal = loaded.connectionModal}
    <ConnectionModal
      isEditing={appState.isEditing}
      bind:modalName={appState.modalName}
      bind:modalHost={appState.modalHost}
      bind:modalUser={appState.modalUser}
      bind:modalPort={appState.modalPort}
      bind:modalUseKerberos={appState.modalUseKerberos}
      bind:modalForwardAgent={appState.modalForwardAgent}
      bind:modalBastion={appState.modalBastion}
      bind:modalBastionUser={appState.modalBastionUser}
      bind:modalKeyPath={appState.modalKeyPath}
      bind:modalTagsString={appState.modalTagsString}
      onClose={() => (appState.showModal = false)}
      onSave={appState.saveConnection}
      onBrowseKey={appState.browseKey}
    />
  {/if}

  {#if pendingPrompt && loaded.connectionChangeModal}
    {@const ConnectionChangeModal = loaded.connectionChangeModal}
    <ConnectionChangeModal
      prompt={pendingPrompt}
      onConfirm={acceptPendingChange}
      onCancel={dismissPendingChange}
    />
  {/if}

  {#if appState.showEnvModal && loaded.envModal}
    {@const EnvModal = loaded.envModal}
    <EnvModal
      environments={appState.environments}
      bind:newEnvName={appState.newEnvName}
      onClose={() => (appState.showEnvModal = false)}
      onCreate={appState.createEnv}
      onDelete={appState.deleteEnv}
    />
  {/if}

  {#if appState.showAgentModal && loaded.agentModal}
    {@const AgentModal = loaded.agentModal}
    <AgentModal
      agentSocket={appState.agentSocket}
      agentKeys={appState.agentKeys}
      onClose={() => (appState.showAgentModal = false)}
      onAddKey={appState.selectAndAddKey}
    />
  {/if}

  {#if appState.showSessionManager && loaded.detachedSessionsModal}
    {@const DetachedSessionsModal = loaded.detachedSessionsModal}
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

  {#if kerberosState.showModal && !appState.showOnboarding && loaded.kerberosModal}
    {@const KerberosModal = loaded.kerberosModal}
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

  {#if appState.showDeleteConfirm && appState.deleteTarget && loaded.deleteConfirm}
    {@const DeleteConfirm = loaded.deleteConfirm}
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

  <ApprovalDialog />

  <Toast />
</div>
{/if}

{#if showShortcutsModal && loaded.shortcutsModal}
  {@const ShortcutsModal = loaded.shortcutsModal}
  <ShortcutsModal show={true} onClose={() => (showShortcutsModal = false)} />
{/if}

{#if showAboutModal && loaded.aboutModal}
  {@const AboutModal = loaded.aboutModal}
  <AboutModal
    show={true}
    workspace={appState.workspace}
    activeEnv={appState.activeEnv}
    onClose={() => (showAboutModal = false)}
  />
{/if}

{#if showBatchExecModal && loaded.batchExecModal}
  {@const BatchExecModal = loaded.batchExecModal}
  <BatchExecModal
    show={true}
    connections={appState.allConnections}
    onClose={() => (showBatchExecModal = false)}
  />
{/if}

{#if showUploadModal && loaded.uploadModal}
  {@const UploadModal = loaded.uploadModal}
  <UploadModal
    show={true}
    connections={appState.allConnections}
    initialConnectionIds={uploadConnectionIds}
    initialRemoteDir={uploadRemoteDir}
    onClose={() => (showUploadModal = false)}
  />
{/if}

{#if showQuitConfirmModal && loaded.quitConfirmModal}
  {@const QuitConfirmModal = loaded.quitConfirmModal}
  <QuitConfirmModal
    activeCount={terminalState.count + terminalState.externalSessionCount}
    onCancel={() => (showQuitConfirmModal = false)}
    onMinimize={async () => {
      showQuitConfirmModal = false;
      try {
        const appWindow = getCurrentWindow();
        await appWindow.hide();
      } catch (err) {
        notify(`Failed to minimize window: ${String(err)}`, "error");
      }
    }}
    onQuit={async () => {
      showQuitConfirmModal = false;
      try {
        await invoke("force_quit_app");
      } catch (err) {
        notify(`Failed to quit application: ${String(err)}`, "error");
      }
    }}
  />
{/if}

{#if showCommandPalette && loaded.commandPalette}
  {@const CommandPalette = loaded.commandPalette}
  <CommandPalette
    open={true}
    connections={appState.allConnections}
    activeTab={appState.activeTab}
    settings={appState.settings}
    onClose={() => (showCommandPalette = false)}
    onSelectTab={appState.handleTabChange}
    onConnectHost={appState.handleConnect}
    onOpenAddModal={appState.openAddModal}
    onOpenBatchExec={() => (showBatchExecModal = true)}
    onOpenUpload={() => openUploadModal(undefined)}
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
    onEditHost={appState.openEditModal}
    onCopyCommand={appState.copyToClipboard}
  />
{/if}

<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Plus, Search, List, LayoutGrid, OctagonX, Layers, ShieldCheck, KeyRound, TerminalSquare } from "lucide-svelte";

  import TitleBar from "$lib/components/TitleBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ConnectionsView from "$lib/components/ConnectionsView.svelte";
  import TerminalsView from "$lib/components/TerminalsView.svelte";
  import HistoryView from "$lib/components/HistoryView.svelte";
  import SettingsView from "$lib/components/SettingsView.svelte";
  import ConnectionModal from "$lib/components/modals/ConnectionModal.svelte";
  import EnvModal from "$lib/components/modals/EnvModal.svelte";
  import AgentModal from "$lib/components/modals/AgentModal.svelte";
  import KerberosModal from "$lib/components/modals/KerberosModal.svelte";
  import DetachedSessionsModal from "$lib/components/modals/DetachedSessionsModal.svelte";
  import DeleteConfirm from "$lib/components/modals/DeleteConfirm.svelte";
  import OnboardingModal from "$lib/components/modals/OnboardingModal.svelte";
  import Toast from "$lib/components/Toast.svelte";

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
    if (appState.connections) appState.selectedHostIndex = 0;
  });

  $effect(() => {
    appState.searchQuery;
    appState.selectedTag;
    appState.loadConnections();
  });

  $effect(() => {
    if (appState.activeTab === "terminals") {
      requestAnimationFrame(() => appState.goToTerminals());
    }
  });

  onMount(() => {
    (async () => {
      await appState.loadData();
      await appState.checkOnboarding();
    })();
    window.addEventListener("keydown", appState.handleGlobalKeydown);

    initTerminalListeners(async () => {
      await appState.loadHistory();
      await appState.loadStats();
    });

    if (appState.settings.monitor_kerberos) {
      startKerberosMonitoring({
        warnMinutes: appState.settings.kerberos_warn_minutes,
        onWarning: (message) => notify(message, "info"),
      });
    }

    let teardownWindow = () => {};
    initWindowState().then((teardown) => {
      teardownWindow = teardown;
    });

    const appWindow = getCurrentWindow();
    void appWindow.onCloseRequested((event) => {
      event.preventDefault();
      void appWindow.hide();
    });

    return () => {
      window.removeEventListener("keydown", appState.handleGlobalKeydown);
      teardownTerminalListeners();
      stopKerberosMonitoring();
      teardownWindow();
    };
  });
</script>

<div class="window-container" class:is-fullscreen={windowState.isFullscreen}>
  <TitleBar activeEnv={appState.activeEnv} onQuit={appState.requestQuitApp} />

  <div class="app-layout" class:collapsed={appState.sidebarCollapsed}>
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
        appState.loadConnections();
      }}
      agentActive={appState.agentActive}
      agentKeys={appState.agentKeys}
      onStartAgent={appState.triggerStartAgent}
      onShowAgentModal={() => (appState.showAgentModal = true)}
      kerberosHealth={appState.kerberosHealth}
      kerberosRemainingLabel={appState.kerberosRemainingLabel}
      kerberosPrincipal={kerberosState.status.principal}
      onShowKerberosModal={openKerberosModal}
      onShowSessionManager={appState.openSessionManager}
      onGoToTerminals={appState.goToTerminals}
      onSearchMostUsed={(name) => (appState.searchQuery = name)}
    />

    <main class="main-panel">
      <header class="topbar">
        <div class="search-section">
          {#if appState.activeTab !== "terminals"}
            <div class="search-container">
              <Search class="search-icon" size={16} />
              <input
                type="text"
                placeholder="Search host, alias, or tag... (Press '/' to focus)"
                bind:value={appState.searchQuery}
                class="search-input"
              />
            </div>

            {#if appState.activeTab === "connections"}
              <div class="view-mode-toggle">
                <button
                  class="toggle-btn"
                  class:active={appState.viewMode === "list"}
                  onclick={() => (appState.viewMode = "list")}
                  title="List View"
                >
                  <List size={16} />
                </button>
                <button
                  class="toggle-btn"
                  class:active={appState.viewMode === "grid"}
                  onclick={() => (appState.viewMode = "grid")}
                  title="Grid View"
                >
                  <LayoutGrid size={16} />
                </button>
              </div>
            {/if}
          {:else}
            <span class="topbar-context-label">Active SSH Sessions</span>
            {#if terminalState.externalSessionCount > 0}
              <button type="button" class="topbar-inline-chip alert" onclick={appState.openSessionManager}>
                <Layers size={13} />
                {terminalState.externalSessionCount} away
              </button>
            {/if}
          {/if}
        </div>

        <div class="topbar-quick-actions">
          {#if terminalState.totalSessionCount > 0}
            <button
              type="button"
              class="quick-chip"
              class:highlight={appState.activeTab !== "terminals"}
              onclick={appState.goToTerminals}
              title="Open terminal tabs"
            >
              <TerminalSquare size={14} />
              <span>Tabs ({terminalState.count})</span>
            </button>
          {/if}

          {#if terminalState.externalSessionCount > 0}
            <button
              type="button"
              class="quick-chip alert"
              onclick={appState.openSessionManager}
              title="Manage background and pop-out sessions"
            >
              <Layers size={14} />
              <span>Sessions ({terminalState.externalSessionCount})</span>
            </button>
          {/if}

          <button
            type="button"
            class="quick-chip"
            class:active={appState.agentActive}
            onclick={() => (appState.agentActive ? (appState.showAgentModal = true) : appState.triggerStartAgent())}
            title="SSH Agent"
          >
            <KeyRound size={14} />
            <span>{appState.agentActive ? `Agent (${appState.agentKeys.length})` : "Agent"}</span>
          </button>

          {#if appState.kerberosHealth !== "unavailable"}
            <button
              type="button"
              class="quick-chip kerberos {appState.kerberosHealth}"
              onclick={openKerberosModal}
              title="Kerberos ticket"
            >
              <ShieldCheck size={14} />
              <span>
                {#if appState.kerberosHealth === "valid" || appState.kerberosHealth === "warning"}
                  {appState.kerberosRemainingLabel}
                {:else if appState.kerberosHealth === "missing"}
                  krb: missing
                {:else}
                  krb: expired
                {/if}
              </span>
            </button>
          {/if}
        </div>

        <div class="topbar-actions">
          {#if terminalState.totalSessionCount > 0}
            <button class="cyber-btn ghost danger" onclick={appState.requestCloseAllSessions}>
              <OctagonX size={16} />
              <span>Close all ({terminalState.totalSessionCount})</span>
            </button>
          {/if}
          <button class="cyber-btn" onclick={appState.openAddModal}>
            <Plus size={16} />
            <span>New Server</span>
          </button>
        </div>
      </header>

      <div class="main-body">
        {#if appState.activeTab === "connections"}
          <div class="main-body-panel is-visible">
            <ConnectionsView
              connections={appState.connections}
              viewMode={appState.viewMode}
              selectedHostIndex={appState.selectedHostIndex}
              copiedId={appState.copiedId}
              justDuplicatedId={appState.justDuplicatedId}
              timezone={appState.settings.timezone}
              onSelectHost={(i) => (appState.selectedHostIndex = i)}
              onConnect={appState.handleConnect}
              onEdit={appState.openEditModal}
              onDelete={appState.deleteConnection}
              onDuplicate={appState.duplicateConnection}
              onCopyCommand={appState.copyToClipboard}
              onRefresh={appState.loadConnections}
              onAddHost={appState.openAddModal}
            />
          </div>
        {/if}

        {#if appState.activeTab === "history"}
          <div class="main-body-panel is-visible">
            <HistoryView history={appState.history} timezone={appState.settings.timezone} />
          </div>
        {/if}

        {#if appState.activeTab === "settings"}
          <div class="main-body-panel is-visible">
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

        {#if appState.showTerminalsPanel}
          <div
            class="main-body-panel"
            class:is-visible={appState.activeTab === "terminals"}
            class:is-docked={appState.activeTab !== "terminals"}
          >
            <TerminalsView
              connections={appState.connections}
              bind:searchQuery={appState.searchQuery}
              onSearchInput={appState.loadConnections}
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

  {#if kerberosState.showModal}
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

  {#if appState.showOnboarding}
    <OnboardingModal
      defaultUser={appState.settings.default_user}
      defaultSshConfigPath={appState.workspace.ssh_config_path || ""}
      configRoot={appState.workspace.config_root}
      onBrowseSshConfig={appState.browseSshConfig}
      onComplete={appState.completeOnboarding}
    />
  {/if}

  <Toast />
</div>

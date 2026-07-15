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

  let unlistenConnect: (() => void) | undefined;

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
        onWarning: (message) => {
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
      void appWindow.hide();
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

    return () => {
      window.removeEventListener("keydown", appState.handleGlobalKeydown);
      teardownTerminalListeners();
      stopKerberosMonitoring();
      teardownWindow();
      unlistenConnect?.();
    };
  });
</script>

<div
  class="flex flex-col flex-1 w-full h-[100dvh] min-h-0 overflow-hidden bg-surface"
  class:is-fullscreen={windowState.isFullscreen}
>
  <TitleBar activeEnv={appState.activeEnv} onQuit={appState.requestQuitApp} />

  <div class="flex flex-1 min-h-0 w-full bg-surface overflow-hidden">
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
      kerberosDefaultRealm={kerberosState.status.default_realm}
      onShowKerberosModal={openKerberosModal}
      onShowSessionManager={appState.openSessionManager}
      onGoToTerminals={appState.goToTerminals}
      onSearchMostUsed={(name) => (appState.searchQuery = name)}
    />

    <main class="flex-1 flex flex-col min-w-0 min-h-0 overflow-hidden">
      <!-- Topbar -->
      <header
        class="relative z-30 shrink-0 h-[var(--topbar-h)] border-b border-border flex items-center justify-between gap-3 px-5 bg-surface"
      >
        <div class="flex items-center gap-2.5 shrink-0 min-w-0">
          {#if appState.activeTab !== "terminals"}
            <div class="relative flex items-center bg-surface-input border border-border rounded-lg px-3 py-1.5 w-[280px] transition-colors duration-150 focus-within:border-border-focus focus-within:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]">
              <Search class="text-muted mr-2 shrink-0" size={16} />
              <input
                type="text"
                placeholder="Search host, alias, or tag... (Press '/' to focus)"
                bind:value={appState.searchQuery}
                onkeydown={(e) => {
                  if (e.key === "Escape") {
                    appState.searchQuery = "";
                    (e.target as HTMLInputElement).blur();
                  }
                }}
                class="bg-transparent border-none text-primary outline-none w-full text-xs font-inherit"
              />
              {#if appState.searchQuery}
                <button
                  type="button"
                  onclick={() => (appState.searchQuery = "")}
                  class="bg-transparent border-none text-muted hover:text-primary cursor-pointer flex p-0.5 rounded-full shrink-0 transition-colors outline-none ml-1"
                  title="Clear search"
                >
                  <X size={12} />
                </button>
              {/if}

              {#if appState.activeTab !== "connections" && appState.searchQuery.trim().length > 0}
                <div
                  class="absolute top-full left-0 mt-1.5 w-[320px] bg-surface-raised border border-border rounded-xl shadow-xl z-50 py-1.5 flex flex-col gap-0.5 max-h-[300px] overflow-y-auto"
                >
                  {#if appState.connections.length > 0}
                    <div class="px-3 py-1 text-[10px] font-bold text-muted uppercase tracking-wider border-b border-border/40 pb-1.5 mb-1 flex items-center gap-1">
                      <Search size={10} />
                      <span>Search Results ({appState.connections.length})</span>
                    </div>
                    {#each appState.connections as conn}
                      <div
                        class="flex items-center justify-between gap-3 px-3 py-1.5 hover:bg-white/[0.03] transition-colors duration-100 group"
                      >
                        <div class="flex flex-col min-w-0">
                          <span class="text-xs font-semibold text-primary overflow-hidden text-ellipsis whitespace-nowrap">{conn.name}</span>
                          <span class="text-[10px] text-muted font-mono overflow-hidden text-ellipsis whitespace-nowrap">
                            {conn.user}@{conn.host}:{conn.port}
                          </span>
                        </div>
                        <button
                          type="button"
                          onclick={async () => {
                            appState.searchQuery = "";
                            await appState.handleConnect(conn);
                          }}
                          class="bg-accent/10 hover:bg-accent text-accent hover:text-white border-none py-1 px-2.5 rounded-md text-[10px] font-semibold cursor-pointer transition-all duration-100 shrink-0"
                        >
                          Connect
                        </button>
                      </div>
                    {/each}
                  {:else}
                    <div class="px-3 py-4 text-center text-xs text-muted">
                      No matching hosts found
                    </div>
                  {/if}
                </div>
              {/if}
            </div>

            {#if appState.activeTab === "connections"}
              <div class="flex border border-border rounded-lg p-0.5 bg-white/[0.02]">
                <button
                  class="bg-transparent border-none text-muted p-1 rounded-md cursor-pointer flex transition-all duration-150 hover:text-secondary class:active={appState.viewMode === 'list' ? 'text-primary bg-white/10' : ''}"
                  class:active={appState.viewMode === "list"}
                  onclick={() => (appState.viewMode = "list")}
                  title="List View"
                >
                  <List size={16} />
                </button>
                <button
                  class="bg-transparent border-none text-muted p-1 rounded-md cursor-pointer flex transition-all duration-150 hover:text-secondary class:active={appState.viewMode === 'grid' ? 'text-primary bg-white/10' : ''}"
                  class:active={appState.viewMode === "grid"}
                  onclick={() => (appState.viewMode = "grid")}
                  title="Grid View"
                >
                  <LayoutGrid size={16} />
                </button>
              </div>
            {/if}
          {:else}
            <span class="text-xs font-semibold text-secondary">Active SSH Sessions</span>
            {#if terminalState.externalSessionCount > 0}
              <button
                type="button"
                class="inline-flex items-center gap-1.25 ml-2.5 py-1 px-2.5 rounded-full border border-accent/25 bg-accent/6 text-accent text-[11px] font-semibold cursor-pointer transition-colors duration-150 hover:bg-accent/12"
                onclick={appState.openSessionManager}
              >
                <Layers size={13} />
                {terminalState.externalSessionCount} away
              </button>
            {/if}
          {/if}
        </div>

        <!-- Quick actions (simplified to avoid duplication) -->
        <div class="flex items-center gap-1.5 flex-1 justify-center min-w-0 overflow-x-auto scrollbar-none">
          {#if terminalState.totalSessionCount > 0}
            <button
              type="button"
              class="inline-flex items-center gap-1.25 py-1.25 px-2.5 rounded-full border border-border bg-white/[0.02] text-muted text-[11px] font-medium cursor-pointer whitespace-nowrap shrink-0 transition-colors duration-150 hover:border-border-hover hover:text-secondary hover:bg-white/[0.04] class:highlight={appState.activeTab !== 'terminals' ? 'border-accent/20 text-accent' : ''}"
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
              class="inline-flex items-center gap-1.25 py-1.25 px-2.5 rounded-full border border-accent/25 bg-accent/6 text-accent text-[11px] font-medium cursor-pointer whitespace-nowrap shrink-0 transition-colors duration-150 hover:border-accent/35 hover:text-accent hover:bg-accent/10"
              onclick={appState.openSessionManager}
              title="Manage background and pop-out sessions"
            >
              <Layers size={14} />
              <span>Sessions ({terminalState.externalSessionCount})</span>
            </button>
          {/if}
        </div>

        <!-- Action buttons -->
        <div class="flex items-center gap-2 shrink-0">
          {#if terminalState.totalSessionCount > 0}
            <button
              class="bg-transparent border border-danger/25 text-red-300 py-1.5 px-3.5 rounded-lg font-semibold cursor-pointer inline-flex items-center gap-1.5 text-xs whitespace-nowrap transition-all duration-150 hover:bg-danger/8 hover:border-danger/40 hover:text-red-200"
              onclick={appState.requestCloseAllSessions}
            >
              <OctagonX size={16} />
              <span>Close all ({terminalState.totalSessionCount})</span>
            </button>
          {/if}
          <button
            class="bg-accent border-none text-white py-1.5 px-3.5 rounded-lg font-semibold cursor-pointer inline-flex items-center gap-1.5 text-xs whitespace-nowrap transition-colors duration-150 hover:bg-accent-hover"
            onclick={appState.openAddModal}
          >
            <Plus size={16} />
            <span>New Server</span>
          </button>
        </div>
      </header>

      <!-- Main Body View Panels -->
      <div class="flex-1 min-h-0 relative overflow-hidden bg-surface">
        {#if appState.activeTab === "connections"}
          <div class="absolute inset-0 flex flex-col min-h-0 overflow-hidden transition-all duration-200 {appState.activeTab === 'connections' ? 'opacity-100 visible pointer-events-auto z-10' : 'opacity-0 invisible pointer-events-none z-0'}">
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
          <div class="absolute inset-0 flex flex-col min-h-0 overflow-hidden transition-all duration-200 {appState.activeTab === 'history' ? 'opacity-100 visible pointer-events-auto z-10' : 'opacity-0 invisible pointer-events-none z-0'}">
            <HistoryView history={appState.history} timezone={appState.settings.timezone} />
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

        {#if appState.showTerminalsPanel}
          <div class="absolute inset-0 flex flex-col min-h-0 overflow-hidden transition-all duration-200 {appState.activeTab === 'terminals' ? 'opacity-100 visible pointer-events-auto z-10' : 'opacity-0 invisible pointer-events-none z-0'}">
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

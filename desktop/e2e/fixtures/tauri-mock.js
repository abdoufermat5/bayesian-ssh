/*
 * Throwaway Tauri 2 IPC mock for Playwright end-to-end tests.
 *
 * There is no Tauri runtime in a browser, so this script defines the minimal
 * globals the frontend touches (`window.__TAURI_INTERNALS__`,
 * `window.__TAURI_EVENT_PLUGIN_INTERNALS__`) plus an in-memory backend for
 * every `invoke(cmd, args)` the app performs. It is injected with
 * `page.addInitScript` before the app module is evaluated (see `app.ts`).
 *
 * ## Recorder
 * Every `invoke` is appended to `window.__IPC_LOG` as `{ cmd, args }`, and
 * `window.__IPC_CALLS(cmd)` returns just the argument objects for one command.
 * Tests assert on IPC payloads through these.
 *
 * ## Knobs (set on `window` before the app boots, or via the fixture options)
 *   window.__ONBOARD        - truthy => `needs_onboarding` returns true.
 *   window.__KRB_MODE       - "none" (tools ok, no ticket) | "notools"
 *                             | default (valid ticket).
 *   window.__MOCK_DETACHED  - array of `{ session_id, connection_name }`.
 *   window.__MOCK_POPOUTS   - array of `{ session_id, connection_name, window_label }`.
 *   window.__BATCH_ENV_WARN - truthy => `get_env_status` reports warnings.
 *   window.__SFTP_DELAY     - ms delay for `list_remote_directory`.
 *   window.__AUDIT_DELAY    - ms delay for `run_security_audit`.
 *   window.__BATCH_DELAY    - ms delay for `run_batch_command` (default 300).
 *
 * ## Command overrides
 *   window.__MOCK_OVERRIDES = { [cmd]: (args) => value }
 * An override replaces the built-in handler (and is also recorded). It may
 * return a value or throw (a string is preserved so the UI surfaces it).
 */
(() => {
  const now = Date.now();
  const iso = (m) => new Date(now - m * 60000).toISOString();

  const seedConnections = () => [
    ["prod-api-01", "10.0.4.12", "deploy", 22, ["prod", "api"], 12, false, "bastion.corp"],
    ["prod-api-02", "10.0.4.13", "deploy", 22, ["prod", "api"], 300, false, "bastion.corp"],
    ["prod-db-primary", "db1.internal.corp", "postgres", 22, ["prod", "db"], 60 * 26, true, null],
    ["staging-web", "staging.example.com", "ubuntu", 2222, ["staging", "web"], 60 * 72, false, null],
    ["build-runner", "ci-runner-3.lan", "ci", 22, ["ci"], null, false, null],
    ["homelab-nas", "192.168.1.20", "admin", 22, ["home"], 60 * 24 * 9, false, null],
    ["gpu-trainer", "gpu-a100.lab.corp", "researcher", 22, ["lab", "gpu"], 45, true, null],
    ["edge-proxy-eu", "eu1.edge.example.net", "root", 22, ["prod", "edge"], null, false, null],
  ].map(([name, host, user, port, tags, used, krb, bastion], i) => ({
    id: "id-" + i,
    name,
    host,
    user,
    port,
    tags,
    use_kerberos: krb,
    bastion: bastion || undefined,
    bastion_user: bastion ? "jump" : undefined,
    key_path: i % 3 === 0 ? "~/.ssh/id_ed25519" : undefined,
    created_at: iso(60 * 24 * 40),
    last_used: used == null ? undefined : iso(used),
  }));

  // Mutable server state; mutations update it so reloads are observable.
  let conns = seedConnections();
  let envs = [
    { name: "default", is_active: true },
    { name: "work", is_active: false },
  ];
  let activeEnv = "default";
  let settings = {
    theme: "zinc",
    auto_start_agent: false,
    kerberos_warn_minutes: 15,
    monitor_kerberos: true,
    default_user: "root",
    default_port: 22,
    timezone: "system",
    enable_sftp: true,
    enable_tunneling: true,
    onboarding_complete: true,
  };
  let onboardingComplete = false;
  const sshKeys = [
    {
      name: "id_ed25519",
      key_type: "ED25519",
      fingerprint: "SHA256:Qx3n0b8kdl2bG9",
      comment: "me@laptop",
      public_key_path: "/home/me/.ssh/id_ed25519.pub",
      private_key_path: "/home/me/.ssh/id_ed25519",
      private_key_permission: "600",
      is_secure: true,
    },
    {
      name: "id_rsa_old",
      key_type: "RSA-2048",
      fingerprint: "SHA256:Zk1uB2mvLx77qa",
      comment: "legacy",
      public_key_path: "/home/me/.ssh/id_rsa_old.pub",
      private_key_path: "/home/me/.ssh/id_rsa_old",
      private_key_permission: "644",
      is_secure: false,
    },
  ];

  const buildHistory = () => {
    const history = conns.slice(0, 6).flatMap((c, i) => [
      {
        connection_name: c.name,
        started_at: iso(i * 90 + 5),
        ended_at: iso(i * 90),
        status: i === 2 ? { Error: "Connection refused" } : "Success",
        exit_code: i === 2 ? 255 : 0,
        duration: 300 + i * 120,
      },
      { connection_name: c.name, started_at: iso(i * 90 + 400), ended_at: iso(i * 90 + 380), status: "Success", exit_code: 0, duration: 1200 },
    ]);
    // Real backend shapes: chrono Duration is [secs, nanos]; statuses are
    // SessionStatus variants. A "Terminated" entry with exit code 0 is a
    // success-like outcome and must render as "Succeeded", not "Failed".
    history.unshift(
      { connection_name: "gpu-trainer", started_at: iso(2), status: "Active" },
      { connection_name: "staging-web", started_at: iso(30), ended_at: iso(12), status: "Terminated", exit_code: 0, duration: [1080, 0] },
      { connection_name: "build-runner", started_at: iso(48), ended_at: iso(47), status: "Terminated", exit_code: 130, duration: [42, 500000000] },
    );
    history.push({ connection_name: "homelab-nas", started_at: iso(60 * 24 * 3), ended_at: iso(60 * 24 * 3 - 95), status: "Disconnected", exit_code: 0, duration: [5700, 0] });
    return history;
  };

  const kerberosStatus = () => {
    const mode = window.__KRB_MODE;
    if (mode === "notools") {
      return { tools_available: false, client_configured: false, has_ticket: false, valid: false, principal: null, suggested_principal: null, default_realm: null, config_path: null, cache_path: null, expires_at: null, renew_until: null, renewable: false, seconds_remaining: null };
    }
    if (mode === "none") {
      return { tools_available: true, client_configured: true, has_ticket: false, valid: false, principal: null, suggested_principal: "me@CORP.EXAMPLE", default_realm: "CORP.EXAMPLE", config_path: "/etc/krb5.conf", cache_path: null, expires_at: null, renew_until: null, renewable: false, seconds_remaining: null };
    }
    return { tools_available: true, client_configured: true, has_ticket: true, valid: true, principal: "me@CORP.EXAMPLE", suggested_principal: "me@CORP.EXAMPLE", default_realm: "CORP.EXAMPLE", config_path: "/etc/krb5.conf", cache_path: "/tmp/krb5cc_1000", expires_at: Math.floor(now / 1000) + 3600 * 5, renew_until: Math.floor(now / 1000) + 3600 * 24 * 6, renewable: true, seconds_remaining: Math.max(0, 3600 * 5 - Math.floor((Date.now() - now) / 1000)) };
  };

  let newId = 100;
  const findConn = (idOrName) => conns.find((c) => c.id === idOrName || c.name === idOrName);

  const handlers = {
    needs_onboarding: () => !!window.__ONBOARD && !onboardingComplete,
    complete_onboarding: (a) => {
      onboardingComplete = true;
      const p = (a && a.payload) || {};
      if (p.profile_name) {
        envs = envs.map((e) => ({ ...e, is_active: e.name === p.profile_name }));
        if (!envs.some((e) => e.name === p.profile_name)) envs.unshift({ name: p.profile_name, is_active: true });
        activeEnv = p.profile_name;
      }
      return p.import_ssh_config ? 8 : 0;
    },
    get_active_env: () => activeEnv,
    list_environments: () => envs.map((e) => ({ ...e, is_active: e.name === activeEnv })),
    set_active_env: (a) => {
      activeEnv = a && a.name;
      envs = envs.map((e) => ({ ...e, is_active: e.name === activeEnv }));
      return null;
    },
    create_environment: (a) => {
      envs = [...envs, { name: a && a.name, is_active: false }];
      return null;
    },
    remove_environment: (a) => {
      envs = envs.filter((e) => e.name !== (a && a.name));
      return null;
    },
    get_workspace_info: () => ({
      active_env: activeEnv,
      config_root: "/home/me/.config/bayesian-ssh",
      env_dir: `/home/me/.config/bayesian-ssh/envs/${activeEnv}`,
      config_path: `/home/me/.config/bayesian-ssh/envs/${activeEnv}/config.json`,
      database_path: `/home/me/.config/bayesian-ssh/envs/${activeEnv}/db.sqlite`,
      ssh_config_path: "/home/me/.ssh/config",
      default_user: "root",
      default_port: 22,
      search_mode: "bayesian",
      log_level: "info",
      auto_save_history: true,
      max_history_size: 1000,
    }),
    get_connections: (a) => {
      let r = conns;
      if (a && a.query) r = r.filter((c) => (c.name + c.host + c.user).includes(a.query));
      if (a && a.tagFilter) r = r.filter((c) => c.tags.includes(a.tagFilter));
      return r;
    },
    add_connection: (a) => {
      const name = (a && a.name) || "untitled";
      const conn = {
        id: "id-new-" + newId++,
        name,
        host: (a && a.host) || "",
        user: (a && a.user) || "",
        port: (a && a.port) || 22,
        tags: (a && a.tags) || [],
        use_kerberos: !!(a && a.kerberos),
        bastion: (a && a.bastion) || undefined,
        bastion_user: (a && a.bastionUser) || undefined,
        key_path: (a && a.keyPath) || undefined,
        created_at: iso(0),
      };
      conns = [...conns, conn];
      return null;
    },
    edit_connection: (a) => {
      conns = conns.map((c) =>
        c.id === (a && a.id)
          ? { ...c, name: (a && a.name) || c.name, host: (a && a.host) || c.host, user: (a && a.user) || c.user, port: (a && a.port) || c.port, tags: (a && a.tags) || c.tags, use_kerberos: !!(a && a.kerberos), bastion: (a && a.bastion) || undefined, bastion_user: (a && a.bastionUser) || undefined }
          : c,
      );
      return null;
    },
    remove_connection: (a) => {
      const target = a && a.idOrName;
      conns = conns.filter((c) => c.id !== target && c.name !== target);
      return null;
    },
    get_stats: () => ({
      total_connections: conns.length,
      most_used: conns[0],
      recently_used: conns.slice(0, 4),
      by_tag: (() => {
        const out = {};
        for (const c of conns) for (const t of c.tags) out[t] = (out[t] || 0) + 1;
        return out;
      })(),
    }),
    get_history: () => buildHistory(),
    load_desktop_settings: () => {
      try {
        const stored = JSON.parse(localStorage.getItem("__mock_settings") || "null");
        if (stored) settings = { ...settings, ...stored };
      } catch {
        /* ignore malformed persistence */
      }
      return settings;
    },
    save_desktop_settings: (a) => {
      if (a && a.settings) {
        settings = { ...settings, ...a.settings };
        try {
          localStorage.setItem("__mock_settings", JSON.stringify(settings));
        } catch {
          /* ignore quota errors */
        }
      }
      return null;
    },
    save_workspace_config: () => null,
    import_ssh_config: () => 2,
    pick_ssh_config_file: () => "/home/me/.ssh/config",
    pick_key_file: () => "/home/me/.ssh/id_ed25519",
    pick_backup_file: () => "/home/me/bssh-backup.enc",
    save_backup_file: () => "/home/me/bssh-backup.enc",
    export_connections_payload: () => "exported",
    import_connections_payload: (a) => {
      if (!a || !a.passphrase) throw "Backup requires passphrase";
      if (a.passphrase !== "pw") throw "Failed to decrypt: wrong Passphrase";
      return 8;
    },
    get_agent_status: () => ({ active: true, socket_path: "/run/user/1000/ssh-agent.sock", keys: ["ED25519 SHA256:abc me@laptop"] }),
    start_agent: () => ({ active: true, socket_path: "/run/user/1000/ssh-agent.sock", keys: ["ED25519 SHA256:abc me@laptop"] }),
    add_key_to_agent: () => null,
    get_app_version: () => "2.4.0",
    update_managed_by: () => null,
    check_update: () => null,
    install_update: () => null,
    get_env_status: () => ({
      ssh_agent_available: !window.__BATCH_ENV_WARN,
      ssh_auth_sock: window.__BATCH_ENV_WARN ? null : "/run/user/1000/ssh-agent.sock",
      kerberos_available: true,
      warnings: window.__BATCH_ENV_WARN ? ["SSH_AUTH_SOCK is not set; key-based hosts may prompt for a passphrase."] : [],
    }),
    count_active_sessions: () => 0,
    ping_all_connections: () =>
      conns.map((c, i) => ({
        connection_id: c.id,
        success: c.name !== "build-runner",
        latency_ms: 8 + i * 3,
      })),
    close_all_ptys: () => 1,
    // Fake PTY lifecycle; set window.__MOCK_DETACHED / __MOCK_POPOUTS.
    spawn_pty: () => null,
    write_pty: () => null,
    resize_pty: () => null,
    seal_session_ui: () => null,
    close_pty: () => null,
    detach_pty: () => null,
    reattach_pty: (a) => ({ session_id: a && a.sessionId, connection_name: "gpu-trainer", buffered_output: "researcher@gpu-a100:~$ nvidia-smi\r\n" }),
    open_terminal_window: () => null,
    focus_terminal_window: () => null,
    dock_popout_session: () => null,
    list_detached_sessions: () => window.__MOCK_DETACHED || [],
    list_popout_sessions: () => window.__MOCK_POPOUTS || [],
    claim_popout_session: (a) => ({ session_id: a && a.sessionId, connection_name: "gpu-trainer", buffered_output: "researcher@gpu-a100:~$ nvidia-smi\r\n" }),
    run_batch_command: async (a) => {
      await new Promise((r) => setTimeout(r, window.__BATCH_DELAY || 300));
      const ids = (a && (a.connectionIds || a.connection_ids)) || [];
      return ids.map((id, i) => {
        const c = conns.find((x) => x.id === id) || conns[0];
        const dryRun = !!(a && (a.dryRun || a.dry_run));
        const fail = !dryRun && c.name === "build-runner";
        return {
          connection_id: c.id,
          name: c.name,
          host: c.host,
          user: c.user,
          is_production: c.tags.includes("prod"),
          stdout: dryRun ? `[dry-run] would run on ${c.user}@${c.host}: ${a && a.command}` : fail ? "" : ` 10:${12 + i}:04 up ${3 + i} days,  2:11,  1 user,  load average: 0.${i}2, 0.31, 0.28`,
          stderr: fail ? "ssh: connect to host ci-runner-3.lan port 22: Connection timed out" : "",
          exit_code: fail ? 255 : 0,
          success: !fail,
          duration_ms: dryRun ? 1 : 140 + i * 37,
        };
      });
    },
    list_ssh_keys: () => sshKeys,
    generate_ssh_key: (a) => {
      sshKeys.push({
        name: a && a.name,
        key_type: a && a.keyType,
        fingerprint: "SHA256:new" + newId++,
        comment: a && a.name,
        public_key_path: `/home/me/.ssh/${a && a.name}.pub`,
        private_key_path: `/home/me/.ssh/${a && a.name}`,
        private_key_permission: "600",
        is_secure: true,
      });
      return null;
    },
    copy_ssh_key_to_target: (a) => `Public key copied to ${a && a.target}`,
    run_security_audit: async () => {
      if (window.__AUDIT_DELAY) await new Promise((r) => setTimeout(r, window.__AUDIT_DELAY));
      return {
        score: 78,
        grade: "B",
        rating: "Good",
        findings: [
          { severity: "critical", title: "Private key world-readable", description: "~/.ssh/id_rsa_old has mode 644.", remediation: "chmod 600 ~/.ssh/id_rsa_old" },
          { severity: "warning", title: "RSA-2048 key in use", description: "Consider migrating to ED25519.", remediation: "ssh-keygen -t ed25519" },
          { severity: "info", title: "Agent forwarding disabled", description: "Good default.", remediation: "" },
        ],
        total_critical: 1,
        total_warning: 1,
        total_info: 1,
      };
    },
    fix_security_permissions: () => 1,
    get_kerberos_status: () => kerberosStatus(),
    renew_kerberos_ticket: () => kerberosStatus(),
    acquire_kerberos_ticket: () => kerberosStatus(),
    list_remote_directory: async (a) => {
      const path = (a && a.remotePath) || "/";
      if (window.__SFTP_DELAY) await new Promise((r) => setTimeout(r, window.__SFTP_DELAY));
      if (a && a.connectionName === "edge-proxy-eu") throw "ssh: connect to host eu1.edge.example.net port 22: Connection timed out";
      if (path === "/root") throw "ls: cannot open directory '/root': Permission denied";
      if (path === "/srv") return [];
      const join = (n) => (path.endsWith("/") ? path + n : path + "/" + n);
      const e = (name, is_dir, size, permissions, modified) => ({ name, path: join(name), is_dir, size, permissions, modified });
      return [
        e(".cache", true, 4096, "drwx------", "Sep 30 11:02"),
        e(".ssh", true, 4096, "drwx------", "Aug 12 09:14"),
        e("app", true, 4096, "drwxr-xr-x", "Oct  7 18:40"),
        e("backups", true, 4096, "drwxr-x---", "Oct  1 03:00"),
        e("logs", true, 4096, "drwxr-xr-x", "Oct  8 08:12"),
        e(".bashrc", false, 3771, "-rw-r--r--", "Mar 14 2025"),
        e(".env", false, 412, "-rw-------", "Oct  2 16:20"),
        e("current", false, 18, "lrwxrwxrwx", "Oct  7 18:40"),
        e("deploy.sh", false, 2290, "-rwxr-xr-x", "Sep 21 10:05"),
        e("docker-compose.yml", false, 1834, "-rw-r--r--", "Sep 21 10:05"),
        e("dump-2026-10-01.sql.gz", false, 482193408, "-rw-r-----", "Oct  1 03:12"),
        e("logo.png", false, 48211, "-rw-r--r--", "Jun  3 14:22"),
        e("README.md", false, 6120, "-rw-r--r--", "Jul 19 12:00"),
      ];
    },
  };

  window.__IPC_LOG = [];
  window.__IPC_CALLS = (cmd) =>
    window.__IPC_LOG.filter((entry) => entry.cmd === cmd).map((entry) => entry.args);
  window.__MOCK_RESET = () => {
    window.__IPC_LOG.length = 0;
  };

  let cbId = 1;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    transformCallback: (cb) => {
      const id = cbId++;
      window["_" + id] = cb;
      return id;
    },
    unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    invoke: async (cmd, args) => {
      window.__IPC_LOG.push({ cmd, args: args === undefined ? null : args });
      if (cmd.startsWith("plugin:event|")) return cbId++;
      if (cmd.startsWith("plugin:window|is_")) return false;
      if (cmd.startsWith("plugin:")) return null;
      const overrides = window.__MOCK_OVERRIDES || {};
      const handler = Object.prototype.hasOwnProperty.call(overrides, cmd) ? overrides[cmd] : handlers[cmd];
      if (!handler) {
        console.warn("[mock] unhandled command", cmd, args);
        return null;
      }
      const result = await handler(args);
      if (result === undefined) return null;
      return JSON.parse(JSON.stringify(result));
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
})();

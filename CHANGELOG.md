# Changelog

All notable changes to Bayesian SSH will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- **bssh Teams: sharing a connection no longer leaves you two copies.** `bssh teams share` (and the desktop's share) turns the connection you shared into the team's connection, instead of adding the team's copy next to it as `name@team`. Your teammates' edits now reach the connection you actually use, behind the confirmation of route changes, and your key path and agent forwarding stay yours. Sharing a connection again adopts a `name@team` copy left by an earlier version and removes it. Like any team connection, a local edit you don't share again is replaced by the vault's version at the next sync. `bssh teams unshare` removes the connection from the team and keeps it on your machine as a local connection.

## [2.13.4] - 2026-10-25

### Added
- **bssh Teams: email verification and mailed invites.** When the server needs proof that you own the account's address, `bssh teams init` mails a code and asks for it on the terminal (three tries; an empty line mails a new one), and `--email-code <CODE>` does the same for scripts. Without a terminal it mails the code and says to rerun with `--email-code`. The desktop app's setup form and the invite onboarding ask for the code too, and can send a new one. `bssh teams invite` now says when the server emailed the invite to the invitee, still prints the token as a fallback, and repeats that your device fingerprint goes to them personally, out of band: the server never chooses what they trust.

### Security
- **A Teams member can no longer silently redirect a shared host and harvest credentials.** Agent forwarding (`ssh -A`) is now its own per-connection setting, `forward_agent`, off by default and only settable locally (`bssh add/edit --forward-agent`, the desktop form, JSON import, `ForwardAgent yes` in ssh-config exports). `-K` (Kerberos TGT delegation) still follows `use_kerberos`. A Teams sync that changes an existing connection's host, port, user, bastion, bastion user or Kerberos flag now records the previous values (`pending_change`), turns agent forwarding off, and refuses to connect until you confirm: interactive CLI asks y/N, non-interactive paths (batch `exec`, uploads, `bssh ai`) fail with `bssh confirm <name>`, and the desktop shows a dialog with old → new before opening the terminal. New team entries need no confirmation and start with forwarding off. Existing local Kerberos connections keep forwarding (`forward_agent` is backfilled for rows with `use_kerberos` and no source); source-managed rows start without it. The interactive Kerberos bastion no longer hard-codes `-p 22`: a stored `host:port` (or `[v6]:port`) is used, otherwise `~/.ssh/config` applies.
- **bssh AI: host tags can no longer be edited into a wider policy.** A tag widens the policy (scope by tag, in-client approval) only when bssh owns it: the connection was added by hand, or its inventory source is listed in the new `ai.trusted_tag_sources`. Teams vault entries and cloud inventories (AWS/GCP, Tailscale, Ansible) let other people change tags, so by default their tags only tighten. Every tag still counts for `approval.two_person`, and the new `approval.two_person_hosts` globs make two-person approval independent of who can edit a source. Denials and `bssh ai explain` name the ignored tags and the setting that trusts the source. This also applies to `bssh ai serve`: its hosts come from the team vault, so a gateway policy scoped by tags needs the team's source in `ai.trusted_tag_sources` (or `scope.hosts`); the gateway warns at startup when that's missing.
- **bssh AI: deny paths can no longer be dodged around.** `run_readonly` now checks the paths a command actually names, not only arguments that start with `/`: input redirections (`cat < /etc/shadow`, `cat 0</etc/shadow`), unexpanded globs (`cat /etc/sha*`, `/etc/shado?`, `/etc/[s]hadow`), relative paths after a literal `cd` (`cd /etc && cat shadow`), `~`/`$HOME` paths (`cat ~/.ssh/id_rsa`) and paths attached to options (`diff --to-file=/etc/shadow`, `-f/etc/shadow`). A path that is only decided once the shell expands a glob, or that is built at run time from a variable (`$DIR/x`), asks for approval instead of running, and grants don't cover it. The default deny list also covers SSH host private keys (`.pub` files stay readable), `krb5.keytab` and `*.keytab`, `/etc/ssl/private/**`, `/etc/wireguard/**`, `/etc/sudoers`, `/etc/gshadow-`, `/etc/security/opasswd`, `/root/**`, Kubernetes and Rancher admin configs, `.kube/config`, `.netrc`, `.pgpass`, `.my.cnf`, `.docker/config.json`, `.git-credentials`, `/proc/*/environ` and `/proc/*/mem`.
- Glob matching in the policy engine is iterative (no exponential backtracking), and `run`, `run_readonly` and `run_many` reject commands longer than 64 KiB before they are analyzed.
- **An agent could approve for itself.** The Claude Code guard hook now blocks `bssh`/`bayesian-ssh` too, not just raw `ssh`/`scp`: anything that reaches a host (`connect`, `exec`, `upload`, `download`, `forward`, `proxy`, and a bare `bssh <name>`) or decides approvals (`ai approve`, `ai grant`, `ai stop`, …) is refused, while read-only subcommands (`list`, `show`, `why`, `history`, `stats`, `doctor`, `ping`, `ai explain/log/watch`) pass. `bssh ai approve` only decides from a terminal (stdin, stdout and stderr on a TTY); a pipe can just `--list` or `--deny`.
- Approvals over the control socket are checked by the broker instead of trusted: a grant is narrowed to the request's tool and exact host, this run, a server-generated id, and at most 8 hours. A two-person request never completes from local approvals (they all come from one machine): at most one is recorded and it still ends "needs Teams (two approvers)".
- bssh AI: the read-only jail runs as the low-privilege `bssh-agent` principal (a certificate whose forced command is the jail) when the host trusts the bssh CA, and bubblewrap now masks the policy's deny paths (the list was always empty before). A jail alone only auto-allows `run_readonly`/`run_many` when it runs as `bssh-agent`; otherwise the analyzer decides. `read_file` resolves symlinks on the host and checks the policy against the real target.
- `transport.strict_host_key_checking` is parsed the same way everywhere: `yes` used to mean strict for the ssh binary but trust-on-first-use for the built-in transport. An unknown value now falls back to `strict` with a warning, `bssh config --strict-host-key-checking` validates its argument, and the desktop terminal's bastion hop follows the configured policy instead of always `accept-new`.

## [2.13.3] - 2026-10-25

### Fixed
- **bssh AI with current Claude Code.** Claude Code 2.1.310 speaks MCP 2026-07-28, which has no `initialize`: the client names itself in each request. bssh only read the client from `initialize`, so these runs recorded no start, showed the agent as `unknown` and couldn't ask in the agent's client. The first request that names a client now starts the run, and its display name is used (`Claude Code 2.1.310`).
- Tool schemas no longer carry non-standard number formats (`uint64`…), which made Claude Code print a warning per field.
- Approval cards say why the policy asks, show what a read that needs approval would read, name the real risk of checks and undo steps, show a diff once, and read `reload nginx.service` for service steps. `nginx -s reload` reads as reloading nginx.
- `bssh ai watch` follows terminal resizes instead of wrapping every line.
- The approval card in `bssh ai watch` counts a plan's file edits: a plan that edits a config and reloads a service no longer reads as only the reload.
- A change plan that stops at a failed step says so, with the earlier steps still done, instead of "not applied".

## [2.13.2] - 2026-10-24

### Added
- **`bssh ai watch`**: one live feed for every running agent. Each call is one aligned line (time, decision, what kind of call, host, command, duration and whether it ran in the read-only jail); runs of allowed reads fold into one line; an approval request opens a compact card in place, answered with `a` or `d`, and Enter shows the full card. Joining late replays what the run did so far. `--run`, `--host` and `--json`.

### Changed
- **bssh AI looks the same everywhere.** Approval cards, `watch`, `log`, `report`, `status`, `explain` and `policy` share one visual language: the decision first, 8-character ids, local times, color only for decisions and risk, plain text when piped or with `NO_COLOR`. `--json` output is unchanged.
- **The approval card says what a change does**, read by bssh's parser rather than the agent: what it creates, moves, copies, deletes, edits or restarts, with variables like `$A` resolved, and whether it deletes anything, uses sudo or the network. Each step is split into its commands; the agent's explanation comes last, labelled as unchecked. Single-key answers with no default: `a`, `d`, `v` for the full text, `g` to allow a single call for 15 minutes.
- **Agents' clients show readable results.** A tool result's text is now a status line and the host's output between markers with a random tag per call, instead of the result's JSON on one line; the full JSON stays in the structured content. Refusals say what happened, that nothing ran, and what to do next.
- `bssh ai log <run> --report` reads like a ticket: who, when, where and totals that add up, then changes, denials, redactions, and recordings as a count with the replay command. `bssh ai log <run>` uses the watch format.
- Every session now records what it connected to (`user@host:port`), and removing a host, or a sync that drops it, keeps its sessions in the history instead of deleting them. The database moves to schema 10; existing sessions keep their host and get their target filled in.

### Fixed
- `bssh connect` to a host that isn't saved failed with `FOREIGN KEY constraint failed` before opening the session. Direct connections work again and are recorded in the session history like any other, under `user@host:port`.

## [2.13.1] - 2026-10-24

### Fixed
- bssh AI: the built-in deny rule `rm -rf /*` refused every `rm -rf` on an absolute path, so `sudo rm -rf /var/www/cache` was denied outright instead of asking for approval. Deny patterns now match paths the way a shell does: `*` and `?` stay within one directory and `**` crosses directories. `rm -rf /*` still refuses `/` and top-level directories such as `/var`; the built-in device rule is now `dd of=/dev/**`, so nested device paths like `/dev/disk/by-id/…` stay denied. The text matcher only applies to commands bssh can't fully parse.

### Changed
- Desktop: the sidebar no longer prints a shortcut number next to each section. The shortcuts still work and are shown in each item's tooltip.

## [2.13.0] - 2026-10-24

### Added
- **bssh AI**: let AI agents (Claude Code, Codex, Cursor, VS Code, Claude Desktop, CI bots) operate your servers without ever holding your keys. `bssh ai mcp` is an MCP server whose tools (`list_hosts`, `observe`, `read_file`, `search_logs`, `run_readonly`, `run`, `service`, `edit_file`, change plans with `propose_change`/`apply_change`/`undo`, `status`, `request_access`) go through a fixed pipeline: a shell parser tags every command (read, write, destructive, privilege, network...), a policy decides allow, ask or deny (never a model), asks wait for a human, output is capped and secrets are redacted before the agent sees them, and every call, decision and approval goes to a signed, hash-chained local audit log. Design: deterministic decisions, a read-only jail on the host, approval-bound certificates.
- **Approvals**: a change plan is approved once for all its steps (diffs, verify and rollback shown); `bssh ai approve` and the desktop app's approval dialog decide from outside the agent; low-risk changes on hosts tagged for it can be confirmed in the agent's own client (MCP elicitation, opt-in); grants allow a kind of call for a while. `bssh ai status`, `bssh ai stop` (kill switch), `bssh ai log [--report]` and `bssh ai replay` show and control runs; the desktop app has a live view of each run with a Stop button.
- **Policies** in `config.json` (`ai.policies`), with built-in `observe`, `operate` and `oncall` profiles: scope by tags, read/write/deny paths, deny rules matched on the parsed command, budgets (calls per minute, changes per run, run TTL) and a circuit breaker. `bssh ai explain '<cmd>'` shows how a command is parsed and decided; `bssh ai policy list|show|test` (a test file of commands with expected decisions, for CI).
- **Host enforcement**: `bssh ai setup` creates a low-privilege `bssh-agent` user (root once, no firewall or sudo changes); reads run in a read-only, no-network jail (Landlock via `setpriv` or the `bssh-jail` helper, or bubblewrap) and are allowed without prompts; `bssh ai ca init|show|trust` sets up a local CA so each approved step runs with a 2-minute certificate whose forced command is exactly that step; `bssh ai doctor` reports each host's enforcement and suggests narrow sudo rules.
- **`bssh ai install <client> [--guard]`** writes the MCP config for Claude Code, Codex, Cursor, VS Code or Claude Desktop, and with `--guard` a Claude Code hook that steers raw `ssh`/`scp` to known hosts towards the bssh tools.
- **Teams agents**: `bssh teams agent add|list|remove|policy|pending|approve|deny` creates unattended agent identities (one seat each) with an admin-signed policy; their requests are relayed end-to-end encrypted to the team's admins, with two-person approval for hosts that need it; AI events join the team's signed audit log (the server sees who and when, never commands or output). `bssh ai serve --agent <team/agent>` is the gateway that serves MCP over HTTP to those agents, with short-lived tokens from `bssh ai token`.
- `bssh exec --timeout <secs> --max-output <bytes>`.

### Changed
- Remote execution has timeouts and output caps in both transports, and a non-interactive mode that fails instead of prompting for a password.
- Teams protocol: new member kind `agent`, signed agent policies in the vault, the approval relay and AI audit kinds (5 to 8) with an actor and an encrypted detail. Existing events and entries verify and decode as before; servers need bssh-cloud 0.5.0 for the new features.

## [2.12.1] - 2026-10-24

### Fixed
- Organization errors are shown in the server's words instead of a generic hint: a non-owner's checkout says "only an owner of the organization can manage billing" (it said "you don't have permission for that in this team"), and removing the last owner says "the organization must keep at least one owner" (it said "that already exists or was already used"). Found by the end-to-end test against the hosted server.
- `bssh teams billing <team> --checkout/--manage` no longer refuses an organization owner who isn't a team admin: the client checked the old "team admin" rule itself; the server, which knows the owners, now decides.
- `bssh org owners remove` names the organization by its remaining owners in its confirmation.

## [2.12.0] - 2026-10-24

### Added
- **Organizations in the desktop app**: the Teams page lists your organizations (labelled by their owners' emails) and opens one to see its owners, add or remove an owner (never the last), its teams and its plan and seats, with Subscribe and Manage billing for owners. A team's "Plan and seats" card says when seats are shared by its organization and sends owners to the organization's checkout and portal; other members see who the owners are. Creating a team offers an organization picker when you own more than one (the oldest by default). A server without organizations hides all of it and the team billing card works as before.
- **Organizations** (`bssh org`): an organization owns teams and, with billing, their subscription and seats. `bssh org list` shows each organization (labelled by its owners' emails), your role, its teams and which of them are on this machine; `bssh org show [<org>]` adds the owners, teams and the plan and seat summary; `bssh org create` creates one; `bssh org owners add|remove <org> <email>` manages owners; and `bssh org billing [<org>]` reports, subscribes (`--checkout [--seats N] [--yearly]`) or opens the customer portal (`--manage`). Seats are shared across an organization's teams: someone in several of them takes one seat, and a pending invite holds one. `<org>` is an id prefix or one of its team names, and optional when you have a single organization. `bssh teams create <name> --org <org>` chooses the organization, and `bssh teams billing` points to the organization's billing. One organization is free per account (`bssh org create` explains the rule when refused). A server without the organization endpoints is reported as needing an update.

## [2.11.0] - 2026-10-23

### Added
- **apt and dnf repositories**: add `https://bssh.abdoufermat.dev/apt` (Debian, Ubuntu) or `/rpm` (Fedora, RHEL, openSUSE) once, then install and update with the system's package manager. Signed with the project's GPG key (RSA 4096): `InRelease`/`Release.gpg`, signed `.rpm` files and `repomd.xml`. Built by the release when `REPO_GPG_KEY` is set and published by the docs site.
- **`bayesian-ssh-cli` package**: the command line without the desktop app, as `.deb` and `.rpm` for amd64 and arm64, for servers and headless machines. It replaces `bayesian-ssh` and the other way round.

### Changed
- New brand: one mark (a prompt chevron and a block cursor on a graphite tile) for the app icons, the desktop title bar and the docs, one outline icon set, and a README banner whose terminal shows the real `bssh connect` picker.
- The installer stops when the latest version is already installed (the CLI, and the desktop app unless `--no-gui`): nothing is downloaded and `sudo` doesn't ask for a password. `--force` reinstalls anyway. When only the desktop app is missing, it says it's adding it.

### Fixed
- Dependency `tokio` 1.52.3 → 1.54.1 for RUSTSEC-2026-0386 (`AsyncFd::new` violating I/O safety).
- On Debian 12, Ubuntu 22.04, RHEL 9 and other systems older than glibc 2.39, every command printed `weak version 'GLIBC_2.39' not found`. Release binaries and packages are now built on Ubuntu 22.04; they need glibc 2.34 or newer and print nothing extra.

## [2.10.3] - 2026-10-23

### Changed
- The CLI is quiet by default: `log_level` now defaults to `warn`, so normal runs print only the command's own output instead of `INFO` tracing lines. Existing configs that still hold the old default `info` are switched to `warn` once, on the next run; after that, `bssh config --log-level info` (or `debug`) is kept as an explicit choice. `RUST_LOG=info` also overrides the configured level for a single run.

### Fixed
- After `exit` in a `bssh connect` session, the prompt came back only at the next key press, and that key went to the closed session. Keystrokes are now read on a plain thread instead of Tokio's blocking pool, whose pending read kept the process alive.
- `bssh show web1` (and `connect`, `edit`, ...) now finds `web-prod-1`: a word prefix followed by the digits of a later name segment matches, like `wp1` and `prod1` already did. The digits must be a whole segment, so `web1` does not match `web-prod-10`.
- `bssh import` and `ssh-config` inventory sources now honour `ProxyJump`, so imported hosts behind a bastion connect through it instead of failing direct. `user@host:port`, plain hosts and aliases defined in the same config are resolved to their HostName/User/Port; `ProxyJump none` imports a direct connection; and a `ProxyCommand ssh -W %h:%p <host>` is treated like `ProxyJump`. Only one bastion is supported: for a chain (`ProxyJump a,b`) the first hop is kept and a warning names the ignored hops. Other `ProxyCommand` forms are not imported and print a warning.
- The desktop command palette (Ctrl+K), the Terminals "New session" launcher, the Files host picker, the Keys pickers and the batch run and upload host lists now always cover every host of the active profile. They no longer shrink to whatever the Hosts search box or tag filter currently shows.
- `bssh exec` on a host reached through a bastion no longer forces a remote terminal. It used to print `Connection to <host> closed.` under `[STDERR]` and merge the command's stderr into stdout; output now matches direct hosts.
- `bssh upload` to a host reached through a bastion (sent over SCP) no longer leaves stale `0 B/0 B` progress rows. SCP's own progress meter is silenced and the host ends on one status line, such as `✔ 20.0 MiB (via SCP)`. SCP errors are now reported in the result instead of being printed over the progress bars.

## [2.10.2] - 2026-10-23

### Changed
- The installer is easier to read: it says what it will do before `sudo` asks for a password (install, update from version X, or reinstall), shows a progress bar for downloads, reports the signature check, and prints errors with what to do next. Colors follow `NO_COLOR` and are off when the output isn't a terminal.
- The installer has a short address: `curl -fsSL https://bssh.abdoufermat.dev/install.sh | bash`. `install.sh` is now attached to each release and covered by the signed `SHA256SUMS`; the docs site serves the latest release's copy.
- Documentation moved to [bssh.abdoufermat.dev](https://bssh.abdoufermat.dev) (source: the bssh-docs repository). The mdBook in `docs/`, `book.toml`, `make docs` and the CI job that published it are gone; the README is now a short landing page that links to the site.
- Maintainer notes kept in the repository: release channels in `RELEASING.md`, the desktop design rules in `desktop/DESIGN-SYSTEM.md`.

## [2.10.1] - 2026-10-23

### Changed
- `bssh teams billing` and the desktop **Plan and seats** card now say why a team is read-only: more members than paid seats (add seats with `--manage` or **Manage billing**, or remove members), or no active subscription (subscribe, or remove members down to the free seats).
- A `past_due` subscription is shown as a failed payment that Stripe is retrying, with a pointer to `--manage`; a subscription set to cancel shows its end date and that the team then falls back to the free plan.

### Fixed
- Dependency `ctutils` 0.4.3 → 0.4.5 for RUSTSEC-2026-0385 (non-constant-time mask generation and `u128` comparisons), pulled in through the crypto stack (`aes-gcm`, `pkcs8`, `ed25519`).
- `bssh teams remove` no longer looks like it failed when the key rotation can't run (devices never approved on this machine): the member is removed server-side, and the message now says so and how to finish the rotation. The devices that would block it are listed before the confirmation.
- Billing and payment-provider errors from the server (`PaymentRequired`, `SeatLimit`, and Stripe failures) are shown verbatim, in the CLI and the desktop app, instead of a generic hint that could be wrong for the actual cause.

## [2.10.0] - 2026-10-23

### Added
- **Revoke a pending invite** (`bssh teams uninvite <team> <email>`): a pending invite holds a seat until it is accepted or expires, so a lost token no longer blocks a seat for 7 days. `bssh teams members` lists pending invites for admins, and the desktop app shows them on the Members tab with **Revoke**. Needs a Teams server with the invite endpoints (bssh-cloud 0.2.0): on an older one, `members` lists no invites and `uninvite` says the server needs an update.
- **Teams plans and seats** (`bssh teams billing <team>`): see a team's plan, seats in use and renewal; admins subscribe (`--checkout [--seats N] [--yearly]`) or manage billing (`--manage`) on Stripe's pages, opened in the browser. The desktop app shows the plan and a seat meter on the team's Members tab, with **Upgrade…** and **Manage billing**. A team over its free seats without an active subscription is read-only, and both the CLI and the app say so.

### Fixed
- `bssh teams share` and `unshare` no longer say "Shared" or "Removed" when the server refused the change (a read-only team, or a key rotation pending): they say the change is queued, above the server's reason.
- `bssh teams billing` no longer shows a renewal date for a cancelled subscription.

## [2.9.0] - 2026-10-22

### Removed
- **Desktop Tunnels view**: it only held port-forward rules in memory and never started a tunnel. Use `bssh forward` (`-L`) and `bssh proxy` (`-D`) from the CLI. The "Tunnels" setting is gone; an old `enable_tunneling` value in desktop settings is ignored. The number keys now run `1`–`7` (Keys is `4`).
- **Desktop grid layout and Ping on the Hosts view** (and "Ping all hosts" in the command palette): the list is the one host layout, and `bssh ping` remains on the CLI.
- **Snippets** in the desktop app (the sidebar entry, the Snippets modal and the "Confirm snippets" setting). Snippets you had saved stay in the app's local storage but are no longer shown; an old `confirm_snippet_execution` value in desktop settings is ignored.

### Added
- **bssh Teams** (`bssh teams ...`): share connections with your team through an end-to-end encrypted vault. The sync server stores only ciphertext; all cryptography is client-side and in this repo (`crates/teams-proto`, `services/teams`). `init` creates your account and device keys (sealed with a passphrase, or `BSSH_TEAMS_PASSPHRASE`); `create`, `share`, `unshare`, `invite`, `join`, `grant`, `sync`, `members` and `remove` manage teams. Team connections are an inventory source of kind `team`, so they show in `bssh list` and `bssh connect` (as `name@team` when a local name is taken), and connecting to one is recorded in the team's signed, hash-chained audit log at the next sync. A team key only ever goes to devices an admin approved by fingerprint, and removing a member rotates it. See the Teams page in the docs.
- **Team audit log** (`bssh teams audit`, and an Audit log section per team in the desktop app): who connected to which shared host, when and from where. Every event's signature and each device's hash chain are verified on the admin's machine; edited, forged, reordered or deleted events are flagged, and the command exits non-zero so it can gate compliance checks.
- **`bssh teams init --invite <TOKEN>`**: account creation can carry the invite token, for Teams servers that only accept invited accounts. Invites (CLI and desktop) now include it, and the desktop app passes it when you paste an invite. Older servers ignore the field.
- **Desktop: share hosts with a team from the Hosts list** (**Share with a team…** or `S`), a team badge and Source column on hosts, source filter chips on Hosts and History, and a link from a team session in History to that team's audit log.
- **Desktop onboarding for invited members**: "I was invited to a team" pastes the invite, sets the device passphrase, joins with the admin's fingerprint and shows this device's fingerprint to send back. The solo path ends with connecting to a first host.
- **Desktop Light theme** (with a light terminal palette), **Match system**, and a row density setting (32/40/48 px).
- **Desktop terminals**: connection progress with `Esc` to cancel, failures that stay in the tab with Retry and Edit host, and tab shortcuts (next/previous, close, reopen closed).
- **Teams in the desktop app**: a Teams page to set up a device, unlock it once per session, create or join teams, share and unshare connections, invite, remove members and sync. Granting a key requires pasting the fingerprint the member sent; the button stays disabled until it matches.

### Changed
- **Desktop keyboard**: `1`–`7` open views in sidebar order, `Ctrl`/`Cmd` + `,` opens Settings, and the host list has verbs (`Shift`+`Enter`, `Ctrl`/`Cmd`+`Enter` for Files, `C`, `E`, `S`, `Delete`; edit moved from `Ctrl`+`E` to `E`). The shortcut sheet, sidebar hints and command palette come from one table. The palette gains Teams, host and Settings-section commands with CLI equivalents.
- **Desktop Settings** regrouped into six sections: Workspace, Connection, Terminal & files, Privacy & history, Appearance, Sources & updates. A type scale, shared empty states and layouts that hold at 1024 px; the sidebar collapses to icons below 1100 px.
- **Desktop connect reuses an open session**: Connect (and `Enter`) switches to the tab already open or connecting to that host; `Shift`+`Enter`, "Connect in new tab", the Terminals launcher and reopening a closed tab always start a new session. Tag chips that don't fit the Hosts toolbar go behind a `+N` menu.
- **Desktop colors**: two themes, Dark and Light (plus Match system), with one blue accent used only for primary actions, focus and selection; neutral grays elsewhere; team, role and source badges are neutral; a muted ANSI palette in terminals. Graphite, Midnight, OLED black and Slate load as Dark, Paper as Light.
- Database schema v9 (applied automatically): Teams tables for the local vault, outbox and audit chain (v8), and for identity, known author keys, approved devices and pending audit events (v9).
- Inventory hosts can carry their own bastion and Kerberos settings; for team sources the vault is authoritative for them.

### Security
- **Native SSH transport upgraded to russh 0.64** (from 0.44), fixing RUSTSEC-2026-0153 and RUSTSEC-2026-0154 (unbounded allocations from a malicious agent or peer). Connections now negotiate post-quantum hybrid key exchange (`mlkem768x25519-sha256`) with servers that support it, and RSA keys sign with SHA-2 (`rsa-sha2-256/512`) when the server advertises it, which OpenSSH 10 requires. Host certificates are checked like plain host keys against `known_hosts` (their embedded key must match), so a certificate never bypasses host verification.

## [2.8.0] - 2026-10-20

### Added
- **Recorded sessions you can search**: opt-in recording of interactive session *output* (never keystrokes) as asciicast v2 files, from the CLI and the desktop app (`bssh config --recording on`, optionally `--recording-tags prod,db`; per session `bssh connect --record/--no-record`). Recordings live next to the database in `recordings/`, created `0600`, capped at 50 MiB each and pruned after 90 days. The plain text is indexed with SQLite FTS5: `bssh history --grep "systemctl restart nginx"` finds which host and when, `bssh history` shows a session id and a recorded marker, and `bssh replay <id> [--speed] [--idle-cap]` plays it back. The desktop History view gains a recorded marker, a search box over recorded output and a replay player.
- **Predictive connect**: ranking now weighs the context a session is started from — the git repository you run `bssh` in, the hour of day, and the host you connected to just before — as smoothed likelihood ratios multiplied into the Bayesian score (clamped to ×0.25–×8, only with enough evidence). `bssh` with no arguments suggests the top 3 connections with a reason and connects on Enter; `bssh why <host>` prints the score factor by factor. The desktop app shows the suggestions in the Connections view and the command palette. Capture can be limited with `bssh config --capture-cwd off` or disabled with `--prediction off`.
- **Live inventory sync**: `bssh source add|list|sync|remove` keeps connections in sync with an ssh config, an Ansible inventory, a Tailscale tailnet, AWS EC2 or GCP Compute (through each tool's own CLI, never a shell). Hosts are matched on their id inside the source, so IP changes update the same connection and keep its history; hosts that disappear are marked stale and only deleted with `--prune`; `--dry-run` previews a sync. `bssh list` and `bssh connect` flag stale hosts, and the desktop Settings gain an "Inventory sources" panel.

### Changed
- Database schema v7 (applied automatically): `sessions` gains `recording_path` and launch-context columns, a `session_text` FTS5 index and a `sources` table are added, and `connections` gains `source_id`, `external_id` and `stale_since`.
- Bayesian search scores are now the plain product of their factors (the former `×100` scale is gone); ordering is unchanged.

## [2.7.0] - 2026-10-13

### Added
- **Multi-host `upload`**: `bssh upload` now takes a comma-separated target list (`web1,web2,db1`) or the same `--all` / `-t/--tag` selectors as `exec`, and transfers to every host in parallel (`-j/--parallel`, default 16). New `--dry-run` previews the resolved remote paths without connecting. A destination ending in `/` (or an existing remote directory) receives the file under its local basename. A shared `bayesian_ssh::services::upload` service exposes the upload to the desktop GUI.
- **Transfer progress bars**: `upload` and `download` show `indicatif` progress (per-host bars with bytes done/total, throughput and ETA) when stderr is a terminal, and a per-host line plus a summary (bytes, duration, failures) otherwise. `upload` exits non-zero when any host fails.

### Changed
- `transfer` command progress now uses human-readable sizes (KiB/MiB/GiB) and a single shared upload path across the CLI and GUI.

### Fixed
- `install.sh` installs the desktop app by default: the one-liner now installs the CLI (`bayesian-ssh` + `bssh`) and the desktop app (`bayesian-ssh-desktop`, menu entry and icon), as the README describes; `--no-gui` installs the CLI only. The old `--desktop` flag (desktop app without the CLI) is removed, the interactive menu offers CLI + desktop or CLI only, every download is verified before anything is installed, the menu entry no longer fails on systems without `/usr/share/applications`, and a missing WebKitGTK 4.1 runtime is reported.
- Recursive uploads reported the size of the current file instead of the whole tree as the progress total (the percentage jumped around and could exceed 100%); progress is now cumulative across the tree and advances per chunk.
- **Desktop Files view showed raw color codes** (`[01;34mboot[0m`) and `..` entries on hosts whose shell aliases `ls` to a colorized version (typical behind interactive bastions): the listing now bypasses aliases with `command ls --color=never` and strips any remaining ANSI escapes. Modification dates in `--time-style=long-iso` listings, previously shown as "Unknown", are parsed again.
- **History showed closed sessions as "Running"**: the desktop app reloaded history on `pty-exit`, before the session's end was written to the database, and sessions ended by quitting/crashing the app or killing a terminal were never closed. The backend now emits `history-updated` once the end is persisted, and `get_history` / `bssh history` close any open session whose process is gone (exit code left unknown rather than reported as a failure). `bssh close --cleanup` uses the same reconciliation.

## [2.6.0] - 2026-10-08

### Added
- **Desktop end-to-end tests**: a Playwright suite (`desktop/e2e`, run with `npm run e2e` or `make e2e`) boots the SvelteKit SPA against a stubbed Tauri IPC layer and covers the shell and shortcuts, hosts (list, filtering, sort, keyboard, row actions, ping, batch entry), the host modal, terminals, SFTP, tunnels, keys, audit, history, settings, modals, command palette, batch execution, snippets, toasts, onboarding, themes, accessibility (axe) plus a 2,000-host performance smoke. CI installs the chromium browser and runs it in the `test-desktop` job.
- **In-app updates**: the desktop app checks **Settings → Updates** for signed releases and installs the `.AppImage` and `bayesian-ssh-desktop` `.deb`/`.rpm` from GitHub in place (open sessions are closed and the app restarts; the artifact signature is verified against the minisign key baked into the build). The snap, `install.sh` binaries, the unified packages and source builds report how they are updated instead.
- **Snap Store package**: an amd64 + arm64 snap (`bayesian-ssh` CLI and `bayesian-ssh.gui`), published to the Snap Store from the `snap` job in releases on a `core24` base. Kerberos stays optional: tickets obtained in the app work as is, and `sudo snap connect bayesian-ssh:kerberos-tickets` reuses host file caches (`KRB5CCNAME=FILE:/tmp/krb5cc_*`).
- **Signed releases**: keyless `cosign` signing of `SHA256SUMS` (`SHA256SUMS.sigstore.json`) plus Sigstore build-provenance attestations for every release asset.
- **`install.sh` verifies the cosign signature** of `SHA256SUMS` when `cosign` is installed, in addition to the checksum check it always performs.
- **Desktop `.rpm` and updater signatures** (`.sig`) are now published in GitHub releases alongside the `.deb` and `.AppImage`.

### Removed
- **Terminal UI (`bssh tui` / `bssh ui`)**: The ratatui dashboard has been removed. Use the CLI commands or the desktop GUI (`bssh desktop`) instead. The `ratatui` dependency is dropped and `tui.log` is no longer written.
- **`packaging.yml` workflow**: removed; unified package building is folded into the release workflow's `packages` job.

### Security
- **Option injection blocked everywhere**: Host/user/bastion values starting with `-` (e.g. `-oProxyCommand=…`) or containing shell metacharacters are rejected at the database write boundary for CLI, GUI and imports; every ssh/sftp/scp argv now puts `--` before the destination, and ProxyCommand fields are shell-quoted and `%`-escaped.
- **Host-key verification**: RSA known_hosts entries never matched (signature algorithm name was compared instead of key type), so changed RSA keys were silently accepted under `accept-new`. `@revoked` keys are now refused, `@cert-authority` lines no longer cause false mismatches, and non-22 ports only match `[host]:port` entries.
- **Path traversal**: Environment names are validated (`bssh env`, `--env`, GUI, backup import) so `../..` can no longer escape the config directory or `remove_dir_all` outside it; SFTP directory downloads skip remote names that are not a single plain component.
- **Restore/backup/export**: `restore` validates the file (`PRAGMA quick_check`) and takes a VACUUM INTO safety backup before an atomic 0600 swap; backups and exports are created 0600 from the start and never clobber existing files.
- **Desktop**: OSC 52 clipboard *reads* from remote hosts are refused; renderer-supplied env names, imported backups and Kerberos principals are validated; slow ssh/sftp/kinit commands no longer block the UI thread.
- **Installer**: `install.sh` verifies release SHA256SUMS, uses a private `mktemp` directory and reads prompts from `/dev/tty`.

### Fixed
- Desktop: the Settings view and the Terminals view with open sessions had no page heading, so the view title disappeared (found by the new e2e suite).
- Desktop: select controls (`CustomSelect`) had no accessible name; screen readers announced them as unlabeled buttons.
- Desktop: edits to workspace settings (OpenSSH config path and defaults) were saved from a stale copy of the workspace object and could silently revert; they now persist the typed values.
- Removed tags reappearing after `edit --remove-tags`, and tags dropped when editing a connection found via fuzzy search or alias.
- `connect`/`add` no longer force port 22 over the stored or configured port; port 0 is rejected.
- `exec` and `ping` exit non-zero when any host fails and cap parallelism at 16 hosts; `close` refuses PIDs ≤ 1 or outside `i32`.
- Logs go to stderr, so `bssh export > file.json` produces valid JSON.
- `import` parses `~/.ssh/config` like OpenSSH (case-insensitive keywords, `=`, quotes, `Match` blocks, multi-pattern `Host`).
- Truncated SFTP reads/uploads are reported as errors instead of replacing files; `history --days` with huge values no longer panics.
- Copied SSH commands use `-J` for plain jump hosts and quote key paths.
- `--tags`, `--add-tags` and `--remove-tags` accept comma-separated lists, as the docs show.
- Desktop: out-of-order terminal output on tab switches, saved terminal font settings ignored, zoom crash, stale search/SFTP results overwriting newer ones, leaked PTY sessions after remote exit, tray menu refresh, and double-moving arrow-key selection.
- Desktop: database, file and process-spawning commands (connections, history, settings, environments, agent/Kerberos status, PTY spawn/close, tray refresh) no longer run on the UI thread; terminal input is queued to a per-session writer thread instead of blocking on the PTY.
- Desktop: terminal output is emitted once, only to the window that owns the session, instead of twice to every window; multi-byte UTF-8 characters split across PTY reads are no longer corrupted; the replay buffer is trimmed in 64 KiB steps instead of on every chunk.
- The database is created, chmod-ed and migrated once per process; later opens only reconnect.
- Desktop: History showed every session as failed and durations as `NaN` (the backend sends `[secs, nanos]`); the main window could not change its background color or un-minimize (missing capabilities).
- Desktop performance: the Hosts list is windowed (2,000 hosts: re-render ≈3.9 s → ≈0.1 s) and the grid renders progressively; the hidden Terminals panel no longer renders output or blinks cursors behind other views; `resize_pty` is sent only when the terminal grid actually changes (no more tmux/vim redraws on tab switches); terminal runtime objects are no longer deep-proxied by Svelte; startup loads run concurrently; the window-resize IPC storm and `backdrop-filter` blurs over the terminal canvas are gone; Kerberos polling backs off when unused or the window is hidden.
- Desktop: secondary text and primary buttons now meet WCAG AA contrast (4.5:1) in every theme; the e2e accessibility check enforces it.

### Changed
- **Desktop redesign**: new design system (tokens, primitives, bundled Inter/JetBrains Mono fonts — remote fonts were blocked by the CSP) and every screen rebuilt on it. The title bar hosts the command search; the sidebar has a profile switcher, grouped navigation and agent/Kerberos status; each view has a single header with one primary action. Themes are renamed Graphite, Midnight, OLED black and Slate (stored ids unchanged).
- Desktop: the Tunnels view no longer shows fake sample rules; rules are local configuration only (no tunnel is started). Snippets can be created, edited and deleted.
- Database opens with a 5 s busy timeout and refuses databases written by a newer schema; migration v4 purges orphan tags/aliases.
- CI runs `cargo fmt --check`, workspace-wide clippy `-D warnings`, all workspace tests, and `svelte-check`; third-party actions are SHA-pinned with least-privilege tokens.
- Integration tests moved to `crates/cli/tests/` (they were never compiled from the virtual-workspace root).
- **Release pipeline**: builds now run on pinned `ubuntu-24.04` / `ubuntu-24.04-arm` runners, and the unified CLI+GUI `.deb`/`.rpm` (previously built by the separate `packaging.yml` workflow) are built by the release's `packages` job so `SHA256SUMS` covers them.
- **Snap**: rebuilt from the published release assets (CLI tarball + desktop `.deb`) on a `core24` base for amd64 and arm64.
- Desktop: the sidebar Kerberos indicator and ticket-expiry notifications only appear once a connection uses Kerberos, so machines that merely have a Kerberos client installed no longer show a red "No ticket" status.

## [2.5.2] - 2026-09-06

### Fixed
- **Desktop UI responsiveness and overflow**: Enforced minimum window constraints (780x520) in Tauri configuration to prevent UI clipping on resize; added responsive window resize handler auto-collapsing the sidebar below 860px; prevented window control overlap on compact screens.
- **Search & filter split-brain state**: Unified topbar search input, sidebar tag filter, and connections view state with two-way bindings.
- **Long identifier handling**: Added full hover tooltips and truncation across connection names, hostnames, usernames, file paths, and tags; prevented grid card height ballooning with tag count limits (`+N more` chips).
- **Responsive views**: Made table columns in Connections, History, and SFTP views hide gracefully on compact viewports; eliminated rigid horizontal minimum widths (`min-w-[760px]`, `min-w-[780px]`); made SFTP path breadcrumbs wrap cleanly.
- **Audit findings command formatting**: Made remediation command snippets selectable, scrollable horizontally (`overflow-x-auto select-text`), and responsive on small window sizes.

### Changed
- **Modal & dialog architecture**: Standardized modals across ConnectionModal, DetachedSessionsModal, BatchExecModal, KeysView, and TunnelStudio with uniform header, body, and footer structure; enlarged connection edit dialog and added live tag chips preview with quick-delete buttons.
- **Design tokens & styling**: Added missing Tailwind theme tokens (`--color-surface-elevated`, `--color-surface-active`, `--color-surface-subtle`, `--color-border-strong`) and `.badge-subtle` utility class.

## [2.5.1] - 2026-09-05

### Security
- **Encrypted backups/exports rebuilt on AES-256-GCM + Argon2id**: The previous homegrown SHA-256 keystream "encryption" has been replaced with a vetted authenticated construction. New payloads use a versioned format (v2); legacy v1 backups still decrypt. Tampered ciphertext, wrong passphrases, and truncated payloads now fail loudly.
- **Kerberos helpers no longer resolve from user-writable paths**: `klist`/`kinit` are only executed from system-owned directories; a planted binary in `~/bin` or a writable `$PATH` entry can no longer harvest Kerberos passwords.
- **Path traversal defence on transfers**: Remote paths containing `..` segments are rejected, and local download targets are written via a temp file + `O_NOFOLLOW` atomic rename, so a pre-existing symlink cannot redirect the write.
- **Duplicate-name protection**: Adding/renaming a connection onto an existing name now fails with a clear error instead of silently overwriting rows (was `INSERT OR REPLACE`).

### Fixed
- **Tag search false positives**: `bssh list -t <tag>`, `bssh search tag:/@<tag>`, and `bssh exec -t <tag>` matched substrings inside the JSON tag array (e.g. `db` matched `dba`). Tags are now a normalized `connection_tags` join table with exact matching.
- **Transfer hang on slow SFTP writers**: A second progress channel added a back-pressure hop that could block uploads indefinitely; progress is now reported inline via an `on_chunk` callback.
- **Non-atomic partial downloads**: Single-file and recursive downloads previously truncated/overwrote the destination on a mid-transfer error; they now write to a temp file and rename only on success.
- **Concurrent `remove` no longer bricks `connect`**: Recency updates on connect are best-effort, so a connect racing a delete no longer fails.
- **Double write on session start**: SSH sessions were INSERTed as `Starting` then updated to `Active`; they are now recorded Active in a single INSERT.
- **`status` queries are now deterministic**: Session outcome filters use a structured `status_kind` column instead of `LIKE '%Error%'` over a JSON blob.
- **SQLite backups are consistent**: `bssh backup` uses `VACUUM INTO` instead of a raw file copy, safe against WAL-mode checkpoints mid-copy.
- **LIKE wildcard injection**: `%`/`_`/`\` in connection-name filters are escaped instead of acting as wildcards.

### Changed
- **Schema migrations tracked**: The database now uses SQLite `user_version` with ordered migrations (v1 → v2 → v3) instead of ad-hoc column checks; existing databases migrate in place.
- **Atomic config writes**: `config.json` is written via temp file + rename so a crash cannot corrupt settings.
- **Broader `klist` date parsing**: Accepts ISO-8601, `%d/%m/%Y`, long/short month-name formats and more, instead of only American `%m/%d/%Y`.
- **Stats & ranking use single queries**: `bssh stats` and Bayesian ranking batch-aggregate session data (one `GROUP BY`) instead of issuing 2×N queries; tag statistics come from one grouped query.
- **Type-safe search field selection**: Column names for search are now an enum, eliminating any string-interpolated SQL path.

### Added
- **Tamper tests for the encrypted format**: roundtrip, wrong-passphrase, flipped-ciphertext, flipped-AAD, and legacy-v1-decryption tests.
- **Path-traversal unit tests** for the transfer guard.

## [2.5.0] - 2026-08-23

### Security
- **Content Security Policy**: The desktop app previously shipped with `csp: null` (no policy). A strict CSP is now enforced (`default-src 'self'`, IPC/asset protocols only, `object-src 'none'`, `frame-ancestors 'none'`), so a renderer compromise can no longer invoke arbitrary Tauri commands or load remote content.
- **ProxyCommand injection fix**: Bastion host, bastion user, and key path are now POSIX shell-quoted before interpolation into `ssh -o ProxyCommand=…` (which runs through `sh -c`). A connection with a malicious bastion/key path could previously execute arbitrary shell commands on the local machine.
- **Connection input validation**: `add_connection`/`edit_connection` reject control characters, whitespace, quotes, shell metacharacters, and `@` in host/user/bastion fields, and control characters in key paths — preventing ssh argv/ProxyCommand breakage and injection via imported or hand-edited connection data.
- **Key generation hardening**: `generate_ssh_key` validates the key name (no paths, no traversal) and whitelists `ssh-keygen` key types.
- **Notification injection fixes**: Desktop notifications sanitize control characters, escape AppleScript string literals (macOS) and PowerShell single quotes (Windows), closing both script-injection vectors.
- **Terminal web links restricted**: Only `http`/`https` URLs from terminal output reach the system browser; `file:`, `smb:`, and other schemes are ignored.

### Fixed
- **UTF-8 panic in detached-session buffer**: `String::drain` panicked when the 512 KB replay-buffer trim cut a multi-byte character (non-ASCII output), killing the PTY reader thread and silently freezing the session. Trimming now clamps to char boundaries.
- **PTY output flooding**: The read loop emitted one IPC event per 4 KB chunk, freezing the UI during high-throughput output (`yes`, `cat bigfile`). Output is now coalesced (32 KB batches, partial reads flush early) and the frontend batches `term.write` calls per frame.
- **Dropped terminal output on pop-out/reattach**: The pop-out window registered its output listener *after* writing the replay buffer, losing output in between; reattach could lose output while the xterm instance was being created. Both races are fixed (listener-first registration; per-tab pending output queue flushed on mount).
- **WebGL context loss left a blank terminal**: The renderer now falls back to the Canvas renderer on context loss instead of going permanently blank.
- **Write-after-dispose crashes**: `term.write`/`term.input` calls are now guarded against terminals disposed concurrently.
- **Mutex poisoning crash**: All PTY session-map locks survive poisoning (`lock_sessions`) instead of `unwrap()`-panicking the whole app.
- **Resize storm**: Terminal fitting is throttled to one fit + resize IPC per animation frame, and hidden tabs are skipped.
- **Ghost tabs**: Detach/pop-out failures now clean up or keep the tab with an explicit error instead of leaving broken state; failed spawns auto-close the error tab after 8 s.
- **Listener init race**: Terminal event listeners can no longer leak or double-register during startup/teardown races.
- **Sequential ping storm**: "Ping All" now pings hosts concurrently (bounded at 16) instead of serially (N × 3 s worst case).

### Changed
- **Design system foundation**: The desktop UI now runs on a documented, token-driven design system (`tokens.css` / `base.css` / `components.css`, see `docs/design/design-system.md`): semantic color tokens (including new `panel`, `running`, `error`, `on-accent`, `overlay`), a disciplined 10–20 px type scale, radius rules, and shared component primitives (buttons, inputs, tags/badges, status dots, kbd, alerts, empty states, skeletons, toasts, modals). Shared chrome (title bars, sidebar, modals, toasts, custom select, loader, shortcut sheet, delete dialog) already consumes the primitives; raw palette colors in views were mapped to semantic tokens.
- **Focus & motion**: All interactive elements now show a visible accent focus ring on keyboard focus; status dots gained a calm pulse; the app loader was de-cluttered (no gradients/glow/bounce) and honors `prefers-reduced-motion`.
- **Terminal palette fidelity**: xterm now also reads the theme's `--text-primary`/`--text-secondary`/`--text-muted`/`--surface-input` variables (they were previously undefined, so the terminal silently fell back to fixed colors).
- **Dead styles fixed**: `bg-surface-card` (used by SFTP/tunnel/snippets panels) and `scrollbar-none` were silently no-ops; both are now defined tokens/utilities.
- **Full ANSI terminal palette**: xterm now uses a complete 16-color palette + selection color derived from the active app theme (previously the CSS variables were undefined and the terminal silently fell back to fixed colors).
- **Linux font fallbacks**: Terminal font stack now includes Ubuntu Mono / DejaVu Sans Mono / Liberation Mono so the terminal looks right on distros without JetBrains Mono.
- **Snippets "Execute in PTY" now actually executes**: The command is sent to the active terminal (with the configured confirmation gate) instead of showing a no-op toast.
- **Copy button fixed**: In the Hosts list, the copy button copies the ssh command (it was wired to Edit); the duplicate-then-edit flow no longer selects the wrong host.

## [2.4.0] - 2026-08-15

### Added
- **Optional SFTP and Tunneling**: Desktop Settings > Features can hide SFTP and tunneling UI when those workflows are unused.
- **Terminal rendering upgrades**: WebGL GPU acceleration, SIXEL/iTerm2 image rendering, Unicode 11, and scrollback export.
- **Terminal settings**: Dedicated settings section with live font preview, plus xterm web-links, in-terminal search, and OSC52 clipboard support.

### Changed
- **CLI TUI modularization**: Split search, models, modal input, overlay rendering, and SFTP task spawning into domain-scoped modules.
- **Shared backend services**: Agent, Kerberos, PTY, security audit, and subprocess SFTP now live in the CLI crate and are reused by the desktop GUI.
- **Desktop UI structure**: Settings panels, modal chrome (`ModalShell`), and connection/session/settings stores are extracted from monolithic views.

### Fixed
- **SFTP over bastion + Kerberos**: Interactive marker-based stdin/stdout path for listings and transfers.
- **Desktop shortcuts**: Keyboard shortcuts and tab navigation activate reliably across views.

## [2.3.0] - 2026-07-25

### Added
- **GUI Multi-Host Batch Execution (`Batch Exec` modal)**: Select any subset of registered hosts, optionally filter by name/tag/user, enter a remote command, and run it in parallel across all targets directly from the desktop UI. Results are displayed per-host in a tabbed output inspector with exit code, duration, stdout/stderr panels, and one-click copy.
- **Batch Exec: Dry-Run Safety Mode** (enabled by default): Preview which hosts would be targeted and what command would be sent — without executing any remote code. Production hosts are highlighted with amber `PROD` badges and a warning banner is shown when live execution targets production servers.
- **Resizable & Fullscreen Batch Exec Modal**: The Batch Exec dialog is now resizable via a drag handle in the bottom-right corner, and can be maximized to fill the entire viewport via the maximize button (⤢) in the title bar. Pressing `Escape` dismisses the dialog from keyboard.
- **SSH Agent & Environment Auto-Discovery on Desktop Launch**: When launched from an app icon (where the shell environment is not inherited), the desktop app now uses a three-strategy bootstrap:
  1. `systemctl --user show-environment` for session-level vars (`DISPLAY`, `DBUS_SESSION_BUS_ADDRESS`, `XDG_RUNTIME_DIR`).
  2. Login-shell `env` output (picks up `.profile`/`.zprofile` exports: nvm, cargo, pyenv, etc.).
  3. Active `/proc` scan + `/tmp/ssh-*/agent.*` glob to locate a running `ssh-agent` socket that is not published to the systemd environment — and write it back to `systemctl --user set-environment` for child processes.
- **Single-Instance Enforcement**: Launching the app a second time brings the existing window to the foreground instead of opening a duplicate.
- **`get_env_status` Tauri command**: Returns `ssh_agent_available`, `ssh_auth_sock`, `kerberos_available`, and a human-readable `warnings` list. The Batch Exec modal uses this to display a **dismissible amber warning banner** when the SSH agent socket is not found at startup.

### Fixed
- **Bastion Host TTY Allocation in GUI subprocess**: Replaced `-J` (ProxyJump) with an explicit `ProxyCommand` that carries `-tt -o RequestTTY=force` on the bastion hop. This ensures the bastion security policy (`[ -t 1 ]` check) is satisfied even when the SSH subprocess is launched from a GUI process where stdin is not a terminal — resolving `"Veuillez ajouter l'option -t à votre commande SSH"` errors.
- **Multiselect unselect regression in Batch Exec**: Replaced the `$effect` length-zero guard (which re-selected all hosts on clear) with an `initialized` boolean flag — allowing users to freely unselect individual hosts or use the Clear button without triggering auto-reselection.
- **Clippy lints**: Fixed `clippy::field_reassign_with_default` in `database/search.rs` and `clippy::items_after_test_module` in `transport/subprocess_impl.rs`.
- **Svelte a11y**: Fixed three accessibility warnings in `BatchExecModal.svelte` — added `tabindex="-1"` to the dialog wrapper, keyboard handler (`Escape`) to the backdrop, and `role="separator"` + `tabindex="0"` to the resize handle.

### Changed
- `build_exec_argv` and `build_shell_argv` now use `ProxyCommand` with forced TTY for all non-Kerberos bastion connections (previously used `-J`).
- SSH agent warning banner in Batch Exec modal is **dismissible** — click ✕ to hide it for the session.

## [2.2.0] - 2026-07-25

### Added
- **SSH Key Management (`bssh key`)**: Subcommand and GUI manager (`KeysView.svelte`) to inspect, list, generate (`Ed25519` / `RSA`), and deploy SSH identity keys to remote servers.
- **Security Auditor (`bssh audit`)**: Comprehensive automated security scanner (`AuditView.svelte`) calculating system grade (`A+` to `F`), security score, and actionable remediation steps.
- **Passphrase-Encrypted Exports & Imports (`bssh export --passphrase` / `import`)**: Database backup export and restore protected with PBKDF2-HMAC-SHA256 and Encrypt-then-MAC AES-256 payload encryption.
- **Dual Binary Build & Installation**: Automatically produces and installs both `bayesian-ssh` and `bssh` binary aliases via `Cargo.toml`, `Makefile`, and `install.sh`.
- **Developer UI/UX Redesign**: High-density Zed/Warp-inspired layout, customizable themes, keyboard shortcuts overlay (`Cmd/Ctrl+/` or `F1`), and dynamic About dialog (`?`).

## [2.1.2] - 2026-07-15

### Added
- **Kerberos client detection**: Desktop app reads `krb5.conf` to detect configured realms, infer default principals, and resolve credential cache paths even when no ticket is present.
- **Branded app icon sync**: Tray, title bar, and favicon now use the custom Bayesian SSH icon consistently.

### Fixed
- **Kerberos Heimdal compatibility**: Fixed false "no ticket" detection when valid tickets use `Credentials cache:` output from Heimdal klist.
- **Modal accessibility**: Resolved Svelte a11y warnings in Kerberos, Detached Sessions, and Onboarding modals; replaced deprecated `<svelte:component>` in Sidebar.
- **Tray icon embedding**: Use dedicated 32×32 compile-time icon asset for reliable Linux system tray display.

## [2.1.1] - 2026-07-14

### Added
- **System Tray Support**: Desktop app minimizes to the system tray instead of quitting on close; tray menu provides Open and Quit actions.
- **Graceful Quit Flow**: Dedicated quit button and confirmation dialog when active terminal sessions are open.
- **Shared Terminal Utilities**: Extracted xterm I/O, clipboard key handling, and Linux IME composition guard into reusable helpers.

### Fixed
- **PTY Terminal Rendering**: Set `TERM=xterm-256color` for spawned PTY sessions so vim, nano, and htop render correctly.
- **PTY Resize**: Implemented `resize_pty` to propagate terminal dimensions to the backend PTY master.
- **Terminal Focus Handling**: Global keyboard shortcuts no longer steal keys when the xterm terminal has focus; active tab auto-focuses on switch.

## [2.1.0] - 2026-07-13

### Added
- **Core Modularization and Refactoring**: Comprehensive codebase-wide refactoring of the CLI parser, TUI state management, and TUI keyboard input dispatcher. Splits monolithic modules into clear, domain-scoped files (`sftp.rs`, `tunnels.rs`, `tabs.rs`, `modals.rs`).
- **Tauri Backend Commands Modularization**: Restructured the monolithic `commands.rs` into a dedicated domain-scoped `commands/` directory resolving connections, PTY terminal spawning, settings, env, ssh-agent, native file dialogues, and configuration imports.
- **Svelte 5 App State custom store**: Extracted reactive UI states, properties, and async Tauri IPC invocations from the presentation page component (`+page.svelte`) into a custom store (`appState.svelte.ts`).
- **Terminal UI/UX Improvements**: Dynamic computed-style terminal themes matching Zinc/OLED/Slate/Cyberpunk backgrounds, auto-retheming on settings update using MutationObservers, keyboard and mouse-wheel font size zoom shortcuts (`Ctrl` + `+` / `-` / `0`), extra viewport padding, custom scrollbars, and active tab glows.
- **Detached GUI Subcommand**: Added `bssh desktop` (and `bssh gui`) command to spawn the Tauri desktop client detached in the background and release prompt control instantly.

### Fixed
- **Terminal Clipboard Handling**: Custom event handlers intercepting copy/paste shortcuts to resolve character scrambling, duplicate pasting, and SIGINT conflicts when copying active selections.

## [2.0.0] - 2026-07-13

### Added
- **Multi-host Split View and Session Management**: Connect to multiple hosts concurrently without closing current terminal sessions. Features a sidebar for quickly searching and spawning terminal sessions in tabs.
- **Popout Terminals & Detached Session Management**: 
  - Move active terminal sessions into standalone windows with full drag-and-drop / tab reattachment support.
  - Custom `DetachedSessionsModal` lists all background/popped-out terminal processes with close, reattach, and kill actions.
- **Kerberos Ticket Management**:
  - Implemented real-time ticket checks, acquisition, and renewal scripts (`kerberos.rs`).
  - Added warning thresholds on ticket expiry and a dedicated interactive `KerberosModal` for quick credential refreshes.
- **App Onboarding Process**: Guided start workflow using an `OnboardingModal` setting up default users, workspace profiles, paths, and themes on first run.
- **Session Logging**: Capture and audit stdout/stderr terminal session logs securely in sqlite history database.
- **SSH Agent & Configuration Defaults**:
  - Automatically detect and pre-fill the custom SSH agent socket from the live `SSH_AUTH_SOCK` environment variable.
  - Option to set a default identity file (SSH private key) globally in Settings.
- **Premium Custom Modals**: Beautiful dialog overlays replacing ugly browser native `confirm()` prompts for connection or profile deletions. Features high-fidelity pulsing warning animations and full dark mode compatibility.
- **Frictionless Connection Cloning**:
  - Fixed arguments parsing issue with `add_connection` on duplicate.
  - Automatically highlights newly duplicated connections using a brief cyan flash animation.
  - Immediately opens the Connection edit drawer upon duplication to prompt for value updates.
- **Aesthetics & Accessibility**: Enhanced sidebar toggle navigation, custom titlebar controls, responsive grid/list styling, and refactored overall project component modularity.

### Fixed
- **PTY Session Isolation**: Kept master PTY descriptor alive in `PtySession` state to prevent closing one tab from triggering EOF and closing all active SSH connections.
- **Manual Close Race**: Suppressed duplicate exit signals when manually closing terminal windows or PTY processes via `close_pty`.

## [1.6.0] - 2026-07-12

### Added
- **Tauri Desktop Application**: A beautiful, performance-oriented standalone desktop GUI built using SvelteKit, TypeScript, and Rust. Features:
  - Vercel/Linear-inspired sleek dark mode layout with Grid and List views.
  - Collapsible drawer sidebar with tags and profiles selection.
  - Interactive tabbed inline terminals powered by Xterm.js and a backend PTY process broker.
  - Connection logs audit table and system metrics monitoring.
  - Global keyboard shortcuts and search bar navigation.
  - Agnostic build and installation system integrated via Makefile and install.sh scripts.
- **Refactored core shared library**: Modularized `bayesian-ssh` into a reusable Rust library target.
- **`doctor` command**: Diagnose the active environment, configuration, SQLite database, SSH client, SSH config, ssh-agent socket, and Kerberos helper tools from the CLI.

### Changed
- **Actionable CLI errors**: Command failures now print a stable `Error:` line and targeted `Suggestion:` hints for common recovery paths.

## [1.5.0] - 2026-04-18

### Added
- **Native russh SSH transport**: Pure-Rust SSH transport layer (`SshTransport` trait) with `russh` backend, known-hosts verification, and multi-method authentication — replaces subprocess-based SSH for supported operations
- **SFTP via russh-sftp**: Full SFTP session implementation (`RusshSftpSession`) built on the native transport for file operations without shelling out
- **`exec`, `upload`, `download` commands**: New CLI commands backed by `TransferService` for remote command execution and file transfers
- **Recursive upload and download**: SFTP upload and download now support entire directory trees recursively
- **Local port-forward tunnel**: `bssh forward -L` establishes SSH local port-forwarding tunnels
- **SOCKS5 dynamic proxy**: `bssh proxy -D` creates a SOCKS5 dynamic forwarding proxy through the SSH connection
- **TUI SFTP file browser**: New Files tab for browsing remote file systems, uploading, deleting, creating directories, and renaming files interactively
- **TUI Tunnels tab**: Start, list, and stop port-forward tunnels directly from the TUI

### Changed
- **Transport architecture refactored**: Extracted `SshTransport` trait with `SubprocessTransport` and `RusshTransport` implementations behind a dispatcher for flexible backend selection

### Fixed
- **CLI argument short flags**: Corrected conflicting short flags for SSH commands

---

## [1.4.1] - 2026-03-05

### Added
- **TUI tab-based navigation**: Three tabs — Connections (`1`), History (`2`), Config (`3`) — switchable with number keys or `Tab`/`Shift+Tab`
- **Add new connection form**: Press `a` in the Connections tab to create a connection directly from the TUI with a 9-field overlay (Name, Host, User, Port, Bastion, Bastion User, Key Path, Kerberos, Tags) and validation
- **Session history view**: History tab with sortable columns (Date, Name, Duration, Status), connection name filter (`/`), failed-only toggle (`f`), and reconnect on `Enter`
- **Environment management**: Config tab listing all environments with the active one highlighted; switch (`Enter`), create (`a`), or delete (`d`) environments without leaving the TUI
- **Connection grouping by tag**: Toggle grouped view with `f` — connections are organized under collapsible tag headers
- **Quick-connect bar**: Press `:` and type `[user@]host[:port]` to connect to an ad-hoc host without saving it
- **Multi-select and batch operations**: `Space` to toggle selection, `Ctrl+A` to select all, `x` to batch-delete selected connections with a confirmation dialog
- **SSH command preview**: Press `p` to see the full SSH command that would be executed, broken down by component (host, port, user, bastion, kerberos flags, key path)
- **Async TCP ping with status indicators**: Press `P` to ping the selected connection in the background; results appear as colored indicators — green `●` with round-trip time for reachable, red `●` for unreachable, yellow `◌` while checking
- **Shared ping service** (`services/ping.rs`): Lightweight TCP-level reachability check using `tokio::net::TcpStream` with timeout — no external process spawning; pings bastion host on port 22 for bastion connections

### Changed
- **TUI modular architecture**: Split monolithic `tui/app.rs` (721 lines) and `tui/ui.rs` (572 lines) into focused modules:
  - `models.rs` — enums and small types (`Tab`, `AppMode`, `EditState`, `PingStatus`, etc.)
  - `state.rs` — `App` struct and state management with `tokio::sync::mpsc` ping channel
  - `input.rs` — keyboard handlers dispatched per tab and mode
  - `event_loop.rs` — terminal setup/teardown and main loop with async ping result draining
  - `ui/mod.rs` — draw dispatcher routing to tab views and overlays
  - `ui/header.rs`, `ui/list.rs`, `ui/detail.rs`, `ui/status.rs`, `ui/overlays.rs`, `ui/history.rs`, `ui/config.rs`, `ui/helpers.rs`
- **Edit overlay extended**: Now supports 9 fields (added Key Path); `EditState` includes `is_new` flag and `validate()` for required-field checks
- **Ping indicators use distinct colors**: Reachable (green), unreachable (red), checking (yellow) — previously all rendered identically

### Fixed
- **Unreachable key binding patterns**: `Ctrl+A` select-all now correctly takes precedence over plain `a` add-connection; removed dead `g` grouping arm that was shadowed by go-to-top

---

## [1.4.0] - 2026-03-05

### Added
- **New commands**: `bssh backup`, `bssh restore`, `bssh duplicate`, `bssh env`, `bssh export`, `bssh groups`, `bssh ping` — expand the CLI with backup/restore, connection cloning, environment management, export, grouping, and latency checks
- **Multi-environment configuration**: Manage separate configs per environment with the `--env` flag; environment name shown in TUI header and logs
- **TUI detail pane**: Side panel (toggled with `Enter`/`d`) displaying all connection fields, full SSH command preview, and contextual hints
- **TUI inline editing**: Edit any connection directly from the TUI with `e` — 8-field overlay with cursor navigation, written back to the database on save
- **TUI sorting**: Cycle sort field with `s` (Name → Host → Last Used → Created) and toggle direction with `S`; indicator shown in header
- **TUI compact view**: Toggle single-line vs two-line row display with `v`
- **Optimized release profile**: `opt-level=3`, thin LTO, single codegen unit, stripped symbols, `panic=abort`

### Changed
- **Database module split**: `database/` refactored into separate submodules (`connection`, `alias`, `session`, `search`) for better organisation
- **Core error handling**: Internal refactoring of error types and propagation across modules
- **TUI visual refresh**: Alternating row backgrounds, `[B]`/`[K]` bastion/Kerberos badges, mode-coloured status bar badge, improved help overlay (50-col popup)

### Fixed
- **TUI log corruption**: Tracing output is now routed to `~/.local/share/bayesian-ssh/tui.log` when the TUI is active, preventing log lines from corrupting the alternate-screen display

### Removed
- **SCP command**: Removed `bssh scp` and the associated config fields `bastion_scp_mode` and `bastion_scp_wrapper`

---

## [1.3.2] - 2025-12-31

### Fixed
- **Log level configuration ignored**: The `log_level` setting in config file was not being applied because logging was initialized before loading the configuration. Now the config is loaded first and the tracing subscriber respects the configured log level (`trace`, `debug`, `info`, `warn`, `error`, `off`/`none`).

## [1.3.1] - 2025-12-30

### Added
- **Bayesian-ranked search**: Smart connection ranking combining:
  - Prior probability (usage frequency with Laplace smoothing)
  - Likelihood (match quality: exact, prefix, word-boundary, contains)
  - Recency (exponential decay based on last use)
  - Success rate (connections that work get boosted)- **Configurable search mode** (`bssh config --search-mode bayesian|fuzzy`)
  - `bayesian`: Smart ranking based on usage patterns (default)
  - `fuzzy`: Simple pattern matching
- **Assets**: SVG icons, banner, architecture and workflow diagrams

### Changed
- Default search mode is now "bayesian" for smarter results

## [1.3.0] - 2025-12-30

### Added- **Interactive TUI mode** (`bssh tui`): Full-screen terminal interface with ratatui
  - Browse, search, and connect to servers
  - Keyboard navigation (vim-style j/k, arrows, PgUp/PgDn)
  - Tag filtering, help overlay, confirmation dialogs
- **Session history command** (`bssh history`): View connection history with statistics
  - Success/failure rates, average duration
  - Filter by connection, days, failed-only
- **Connection aliases** (`bssh alias`): Create shortcuts for connections
  - `bssh alias add db prod-database` → `bssh connect db`
  - Aliases work transparently with connect command
- **Close/kill sessions** (`bssh close`): Manage active SSH sessions
  - List active sessions with PID and stale detection
  - Close specific sessions or all at once
  - `--cleanup` to remove stale sessions (PIDs no longer running)
- **Configurable search mode** (`bssh config --search-mode bayesian|fuzzy`)
  - `bayesian`: Smart ranking based on usage patterns (default)
  - `fuzzy`: Simple pattern matching

### Changed
- Connect command now checks aliases before fuzzy search
- Session tracking improved with accurate active/stale detection
- Default search mode is now "bayesian" for smarter results

### Dependencies
- Added `ratatui` and `crossterm` for TUI
- Enabled `signal` feature in `nix` for process management

## [1.2.0] - 2025-12-22

### Added
- **`--force` flag for remove command**: Skip confirmation prompt with `-f` or `--force`
- **`--clear-bastion` flag for config command**: Clear default bastion settings
- **Shared CLI utilities module**: New `src/cli/utils.rs` for consistent UX across commands
- **Working "search again" feature**: The 's' option in interactive selection now performs actual recursive search
- **Contextual help messages**: Suggestions like "Use 'bssh list' to see all connections" when no matches found

### Changed
- **Single-match auto-connect**: Connect and show commands now auto-select when only one fuzzy match is found (improved UX)
- **Default yes for non-destructive operations**: Confirmation prompts now default to Yes `[Y/n]` for show/connect
- **Simplified remove confirmation**: Changed from typing full connection name to simple y/n prompt (use `--force` to skip)

### Fixed
- **Config update bug**: Fixed double-wrapping of `Option<Option<String>>` for bastion settings that prevented clearing values
- **DateTime parsing panic**: Graceful handling of malformed dates in database instead of crashing
- **Home directory panic**: Better error message when `$HOME` is not set during SSH config import

### Technical
- **Major code deduplication**: Extracted ~300 lines of duplicated code from connect.rs, edit.rs, remove.rs, show.rs into shared utilities
- **Reduced file sizes**: connect.rs (257→65 lines), edit.rs (360→70 lines), remove.rs (235→70 lines), show.rs (246→35 lines)

## [1.1.1] - 2025-08-29

### Fixed
- **Configuration defaults**: Changed default user from hardcoded "admin" to current system user
- **Kerberos default**: Disabled Kerberos by default (changed from `true` to `false`)
- **Documentation**: Updated all examples to reflect new sensible defaults

### Changed
- **Default configuration**: Application now uses current Linux username instead of "admin"
- **Kerberos behavior**: Kerberos authentication is now opt-in rather than default
- **Documentation examples**: Updated configuration commands and JSON examples across all docs

### Technical
- **Dependencies**: Added `whoami` crate for system user detection
- **Configuration**: Updated `AppConfig::default()` implementation
- **Documentation**: Updated README.md, docs/README.md, and docs/advanced-usage.md

## [1.1.0] - 2025-08-28

### Added
- **Intelligent fuzzy search across all commands** - Find connections by partial names, tags, or patterns
  - Enhanced `connect` command with fuzzy search and interactive selection
  - Enhanced `edit` command with fuzzy search for connection management
  - Enhanced `show` command with fuzzy search for connection details
  - Enhanced `remove` command with fuzzy search and extra confirmation

### Fuzzy Search Features
- **Smart pattern matching**: Handles hyphens, underscores, and separators (`webprod` → `web-prod-server`)
- **Tag-based search**: Search within connection tags
- **Recent connections fallback**: Shows recently used connections when no matches found
- **Interactive selection**: Numbered menus for multiple matches with user-friendly prompts
- **Relevance ranking**: Prioritizes recently used and exact matches

### Enhanced Safety
- **Extra confirmation for destructive operations**: `remove` command requires typing full connection name
- **Graceful error handling**: Clear messages and helpful suggestions
- **Backwards compatibility**: All existing functionality preserved

### Documentation
- Updated README with fuzzy search examples across all commands
- Enhanced user guide with practical usage scenarios
- Improved feature descriptions and examples

### Technical Improvements
- Enhanced database layer with fuzzy search algorithms
- Improved error handling and user feedback
- Better code organization and maintainability

## [1.0.0] - 2024-08-23

### Added
- Initial release of Bayesian SSH
- Basic SSH connection management
- Kerberos authentication support
- Bastion host routing
- Tag-based organization
- SQLite database persistence
- Connection history and statistics

### Core Features
- One-click connections to servers
- Automatic Kerberos ticket management
- Smart bastion host routing
- Tag-based organization for easy management
- Complete connection history with statistics
- SQLite database for persistence

---

## Types of changes
- `Added` for new features
- `Changed` for changes in existing functionality
- `Fixed` for any bug fixes
- `Removed` for now removed features
- `Security` in case of vulnerabilities

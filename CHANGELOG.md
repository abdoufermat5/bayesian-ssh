# Changelog

All notable changes to Bayesian SSH will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

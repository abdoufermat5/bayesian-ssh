# TUI Mode

Launch a full-screen terminal interface for browsing and managing connections, sessions, environments, files, and tunnels:

```bash
bayesian-ssh tui
```

## Tabs

The TUI is organized into five tabs. Switch with number keys or `Tab` / `Shift+Tab`.

| Key | Tab | Purpose |
|-----|-----|---------|
| `1` | **Connections** | Browse, search, edit, and connect to saved hosts |
| `2` | **History** | Inspect past sessions and reconnect |
| `3` | **Config** | Switch, create, or delete environments |
| `4` | **Files** | SFTP file browser on the selected host |
| `5` | **Tunnels** | Start, list, and stop SSH port-forward tunnels |

## Global Keybindings

| Key | Action |
|-----|--------|
| `1` … `5` | Jump to tab |
| `Tab` / `Shift+Tab` | Next / previous tab |
| `?` | Help overlay |
| `q` | Quit |
| `r` | Refresh current tab |

## Connections Tab

| Key | Action |
|-----|--------|
| `↑` / `k` · `↓` / `j` | Navigate |
| `PgUp` / `PgDn` | Page navigation |
| `Enter` | Connect to selected host |
| `d` | Toggle detail pane |
| `/` | Filter by name |
| `t` | Filter by tag |
| `f` | Toggle grouping by tag |
| `s` / `S` | Cycle sort field / toggle direction |
| `v` | Toggle compact / two-line rows |
| `a` | Add a new connection (9-field form) |
| `e` | Edit the selected connection |
| `p` | Preview the SSH command that would run |
| `P` | Async TCP-ping the host (shows `●` indicator) |
| `Space` | Toggle multi-select on the row |
| `Ctrl+A` | Select all |
| `x` | Batch-delete selected connections (with confirmation) |
| `:` | Quick-connect bar — type `[user@]host[:port]` for an ad-hoc session |

### Detail Pane

Press `d` (or `Enter` after enabling) to open a side panel showing all connection fields, the full SSH command that would be executed, and contextual hints.

### Add / Edit Form

Press `a` for a new connection or `e` to edit the selected one. The 9-field overlay covers Name, Host, User, Port, Bastion, Bastion User, Key Path, Kerberos, and Tags. Required fields are validated on save.

### Ping Indicators

Press `P` to TCP-ping the selected host in the background:

- 🟢 **Green ●** — reachable, with round-trip time
- 🔴 **Red ●** — unreachable
- 🟡 **Yellow ◌** — check in progress

For bastion connections, the bastion host is pinged on port 22.

### Visual Indicators

- `[B]` badge — bastion host is configured
- `[K]` badge — Kerberos is enabled
- Alternating row backgrounds for readability
- Mode-coloured status-bar badge

## History Tab

Inspect past SSH sessions with sortable columns and reconnect on demand.

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate sessions |
| `Enter` | Reconnect to the session’s host |
| `s` | Cycle sort column (Date, Name, Duration, Status) |
| `/` | Filter by connection name |
| `f` | Toggle “failed only” |

## Config Tab

Manage environments without leaving the TUI. The active environment is highlighted.

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate environments |
| `Enter` | Switch to the selected environment |
| `a` | Create a new environment |
| `d` | Delete the selected environment (with confirmation) |

## Files Tab

Browse the remote file system of the selected host over SFTP.

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate entries |
| `Enter` | Enter directory |
| `Backspace` | Go up one directory |
| `u` | Upload a local file |
| `g` | Download (get) the selected file |
| `n` | Create a new directory |
| `r` | Rename the selected entry |
| `D` | Delete the selected entry (with confirmation) |

## Tunnels Tab

Start and manage SSH port-forward tunnels.

| Key | Action |
|-----|--------|
| `a` | Open “new tunnel” form (local port, remote host, remote port) |
| `Enter` | Start the highlighted tunnel |
| `x` | Stop the selected tunnel |

## TUI Logging

When the TUI is active, tracing output is routed to `~/.local/share/bayesian-ssh/tui.log` to prevent log lines from corrupting the alternate-screen display.
# TUI Mode

Launch a full-screen terminal interface for browsing and managing connections:

```bash
bayesian-ssh tui
```

## Keybindings

| Key | Action |
|-----|--------|
| `Up` / `k` | Navigate up |
| `Down` / `j` | Navigate down |
| `PgUp` / `PgDn` | Page navigation |
| `Enter` | Connect / Toggle detail pane |
| `/` | Search |
| `t` | Filter by tag |
| `d` | Toggle detail pane |
| `e` | Edit connection inline |
| `s` | Cycle sort field (Name, Host, Last Used, Created) |
| `S` | Toggle sort direction |
| `v` | Toggle compact/expanded view |
| `r` | Refresh |
| `?` | Help overlay |
| `q` | Quit |

## Features

### Detail Pane

Press `Enter` or `d` on a connection to open a side panel showing:
- All connection fields
- Full SSH command preview
- Contextual hints

### Inline Editing

Press `e` to edit any connection directly from the TUI:
- 8-field overlay with cursor navigation
- Changes are written back to the database on save

### Sorting

- Press `s` to cycle through sort fields: Name, Host, Last Used, Created
- Press `S` to toggle ascending/descending order
- The current sort indicator is shown in the header

### Compact View

Press `v` to toggle between:
- **Single-line rows** - compact view showing key info
- **Two-line rows** - expanded view with more detail

### Visual Indicators

- `[B]` badge indicates bastion host is configured
- `[K]` badge indicates Kerberos is enabled
- Alternating row backgrounds for readability
- Mode-coloured status bar

## TUI Logging

When the TUI is active, tracing output is routed to `~/.local/share/bayesian-ssh/tui.log` to prevent log lines from corrupting the terminal display.

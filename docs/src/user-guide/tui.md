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

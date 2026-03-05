# Quick Start

## Add Your First Server

```bash
bayesian-ssh add "My Server" server.company.com
```

## Connect to It

```bash
bayesian-ssh connect "My Server"
```

## Browse All Connections

```bash
# List view
bayesian-ssh list

# Interactive TUI
bayesian-ssh tui
```

## Core Commands at a Glance

| Command | Description |
|---------|-------------|
| `bayesian-ssh add` | Add a new connection |
| `bayesian-ssh connect` | Connect to a server (fuzzy search) |
| `bayesian-ssh list` | List all connections |
| `bayesian-ssh show` | Show connection details |
| `bayesian-ssh edit` | Edit a connection |
| `bayesian-ssh remove` | Remove a connection |
| `bayesian-ssh import` | Import from SSH config |
| `bayesian-ssh tui` | Launch interactive TUI |
| `bayesian-ssh history` | View session history |
| `bayesian-ssh alias` | Manage connection aliases |
| `bayesian-ssh config` | View/update configuration |
| `bayesian-ssh stats` | View statistics |
| `bayesian-ssh close` | Manage active sessions |
| `bayesian-ssh backup` | Backup database |
| `bayesian-ssh restore` | Restore from backup |
| `bayesian-ssh ping` | Check server latency |

## Fuzzy Search

All commands support intelligent fuzzy search:

```bash
bayesian-ssh connect "webprod"     # Finds "web-prod-server"
bayesian-ssh connect "prod"        # Shows all production servers
bayesian-ssh show "dbprod"         # Show connection details
bayesian-ssh edit "apigateway"     # Edit connection settings
```

Search is Bayesian-ranked by default, combining:
- **Usage frequency** (with Laplace smoothing)
- **Match quality** (exact, prefix, word-boundary, contains)
- **Recency** (exponential decay based on last use)
- **Success rate** (connections that work get boosted)

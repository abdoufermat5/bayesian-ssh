<p align="center">
  <img src="assets/banner.svg" alt="Bayesian SSH Banner" width="100%">
</p>

# Bayesian SSH - Fast and Easy SSH Session Manager

[![Rust](https://img.shields.io/badge/Rust-1.70+-blue.svg)](https://rustup.rs/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![CI](https://github.com/abdoufermat5/bayesian-ssh/workflows/CI/badge.svg)](https://github.com/abdoufermat5/bayesian-ssh/actions/workflows/ci.yml)

> **An ultra-fast and intelligent SSH session manager with Bayesian-ranked search, fuzzy matching, Kerberos support, bastion hosts, and advanced history management.**

## What is Bayesian SSH?

**Bayesian SSH** transforms your SSH experience with intelligent automation:

- **Bayesian-ranked search** - connections ranked by frequency, recency, and match quality
- **Intelligent fuzzy search** across all commands - find connections by partial names, tags, or patterns
- **One-click connections** to your servers
- **Automatic Kerberos** ticket management
- **Smart bastion host** routing
- **Tag-based organization** for easy management
- **Complete connection history** with statistics
- **SQLite database** for persistence

## Quick Start

### Installation

#### Option 1: One-liner Install (Recommended)
```bash
# Install latest release automatically (non-interactive)
curl -fsSL https://raw.githubusercontent.com/abdoufermat5/bayesian-ssh/main/install.sh | bash
```

```
# Interactive installation (choose options)
curl -fsSL https://raw.githubusercontent.com/abdoufermat5/bayesian-ssh/main/install.sh | bash -s -- --interactive
```

#### Option 2: Manual Build
```bash
# Clone and build
git clone https://github.com/abdoufermat5/bayesian-ssh.git
cd bayesian-ssh

# Build and install using Makefile
make release
make install

# Or use the script
./scripts/build_and_push.sh --release
sudo cp target/release/bayesian-ssh /usr/local/bin/
```

### First Connection
```bash
# Add a server
bayesian-ssh add "My Server" server.company.com

# Connect instantly
bayesian-ssh connect "My Server"
```

## 📖 Basic Usage

### Core Commands
```bash
# Connect to a server (with fuzzy search)
bayesian-ssh connect "Server Name"        # Exact match
bayesian-ssh connect "webprod"            # Finds "web-prod-server"
bayesian-ssh connect "prod"               # Shows all production servers

# Manage connections (all with fuzzy search)
bayesian-ssh edit "webprod"               # Edit connection settings
bayesian-ssh show "dbprod"                # Show connection details
bayesian-ssh remove "apigateway"          # Remove connection

# Add new connection
bayesian-ssh add "Server Name" hostname.com

# List connections
bayesian-ssh list

# Import from SSH config
bayesian-ssh import

# Interactive TUI mode
bayesian-ssh tui                          # Full-screen connection browser
```

### Session Management
```bash
# View session history with stats
bayesian-ssh history                      # Recent sessions
bayesian-ssh history -c "prod"            # Filter by connection
bayesian-ssh history --days 7 --failed    # Last week's failures

# Manage active sessions
bayesian-ssh close                        # List active sessions
bayesian-ssh close "Server"               # Close specific session
bayesian-ssh close --cleanup              # Clean stale sessions
bayesian-ssh close --all                  # Close all sessions
```

### Connection Aliases
```bash
# Create shortcuts for connections
bayesian-ssh alias add db prod-database   # 'db' → 'prod-database'
bayesian-ssh alias add p1 Portail01       # Quick alias
bayesian-ssh connect db                   # Uses alias

bayesian-ssh alias list                   # Show all aliases
bayesian-ssh alias remove db              # Remove alias
```

### Bastion Management
```bash
# Use default bastion
bayesian-ssh add "Server" host.com

# Force direct connection
bayesian-ssh add "Server" host.com --no-bastion

# Custom bastion
bayesian-ssh add "Server" host.com --bastion custom-bastion.com
```


### Configuration

The app automatically creates configuration in `~/.config/bayesian-ssh/`:

```bash
# View current config
bayesian-ssh config

# Set defaults (Kerberos is disabled by default, current user is used)
bayesian-ssh config --use-kerberos --default-user customuser
```

## Documentation

For comprehensive guides and advanced topics, see the [docs/](docs/) folder:

- **[Technical Architecture](docs/technical-architecture.md)** - System design and architecture
- **[Advanced Usage](docs/advanced-usage.md)** - Enterprise scenarios and complex use cases  
- **[Troubleshooting](docs/troubleshooting.md)** - Common issues and solutions
- **[Documentation Index](docs/README.md)** - Complete documentation overview

## Changelog
See [CHANGELOG.md](CHANGELOG.md) for detailed release notes.

##  Contributing

1. **Fork** the project
2. **Create** a feature branch (`git checkout -b feature/AmazingFeature`)
3. **Commit** your changes (`git commit -m 'Add AmazingFeature'`)
4. **Push** to the branch (`git push origin feature/AmazingFeature`)
5. **Open** a Pull Request

##  License

This project is licensed under **MIT**. See the [LICENSE](LICENSE) file for details.

---

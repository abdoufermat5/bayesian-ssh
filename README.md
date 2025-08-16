# 🚀 Bayesian SSH - Intelligent SSH Session Manager

[![Rust](https://img.shields.io/badge/Rust-1.70+-blue.svg)](https://rustup.rs/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Version](https://img.shields.io/badge/Version-0.1.0-orange.svg)](Cargo.toml)
[![CI](https://github.com/asadiakhou/bayesian-ssh/workflows/CI/badge.svg)](https://github.com/asadiakhou/bayesian-ssh/actions/workflows/ci.yml)
[![Security](https://github.com/asadiakhou/bayesian-ssh/workflows/Security/badge.svg)](https://github.com/asadiakhou/bayesian-ssh/actions/workflows/security.yml)
[![Release](https://github.com/asadiakhou/bayesian-ssh/workflows/Release/badge.svg)](https://github.com/asadiakhou/bayesian-ssh/actions/workflows/release.yml)

> **An ultra-fast and intelligent SSH session manager with Kerberos support, bastion hosts, and advanced history management.**

## 🎯 Project Goal

**Bayesian SSH** is a modern tool that transforms your SSH experience with:

- 🚀 **Ultra-fast connections** in one click
- 🔐 **Automatic Kerberos support** with forwardable ticket creation
- 🚪 **Intelligent bastion management** (jump hosts)
- 📊 **Complete history** with tags and statistics
- 🏷️ **Tag-based organization** for efficient management
- 💾 **SQLite database** for persistence
- 🎨 **Intuitive and modern CLI interface**

## ✨ Main Features

### 🔐 **Intelligent Kerberos Authentication**
- ✅ **Automatic verification** of existing tickets
- 🆕 **Automatic creation** of forwardable tickets if needed
- 🔄 **Automatic renewal** of expired tickets
- 🎯 **Support for flags** `-t -A -K` for SSH

### 🚪 **Bastion Host Management (Jump Hosts)**
- 🌐 **Bastion connection** : `ssh user@bastion target@destination`
- 🔧 **Default configuration** for your environment
- 📝 **Connection history** with bastion
- 🎛️ **Command line override** if needed
- 🚫 **Force direct connection** with `--no-bastion` flag

### 🏷️ **Advanced Tag System**
- 🏷️ **Flexible organization** : `production`, `staging`, `web`, `database`
- 🔍 **Search and filtering** by tags
- 📊 **Tag-based statistics** for analysis
- 🎯 **Automatic import** from SSH config

### 💾 **Intelligent Database**
- 🗄️ **Integrated SQLite** for persistence
- 📈 **Complete history** of connections and sessions
- 🔄 **Automatic timestamp updates**
- 📊 **Detailed usage statistics**

## 🚀 Installation

### Prerequisites
- **Linux** (Ubuntu/Debian/Fedora/Arch)
- **Rust** 1.70+ with Cargo
- **SSH client** installed
- **Kerberos** (optional, for authentication)

### Compilation
```bash
# Clone the project
git clone https://github.com/yourusername/bayesian-ssh.git
cd bayesian-ssh

# Build in release mode
cargo build --release

# The executable will be in target/release/bayesian-ssh
```

### Global installation (optional)
```bash
# Copy to PATH
sudo cp target/release/bayesian-ssh /usr/local/bin/

# Or create an alias
echo 'alias bssh="bayesian-ssh"' >> ~/.bashrc
source ~/.bashrc
```

### 🎯 **Enable Tab Completion**
```bash
# Generate completion scripts
./target/release/bayesian-ssh completions bash > bayesian-ssh-completion.bash
./target/release/bayesian-ssh completions zsh > bayesian-ssh-completion.zsh

# Enable for current session
source bayesian-ssh-completion.bash  # or .zsh for zsh

# Enable permanently
echo 'source ~/path/to/bayesian-ssh-completion.bash' >> ~/.bashrc
source ~/.bashrc

# System installation (recommended)
sudo cp bayesian-ssh-completion.bash /etc/bash_completion.d/bayesian-ssh
sudo cp bayesian-ssh-completion.zsh /usr/local/share/zsh/site-functions/_bayesian-ssh
```

## 📖 Usage Guide

### 🎯 **Main Commands**

#### **1. Connect to a server**
```bash
# Simple connection by name
bayesian-ssh connect "My Server"

# Connection with custom parameters
bayesian-ssh connect server-name --user admin --port 2222 --kerberos

# Connection via bastion
bayesian-ssh connect server-name --bastion bastion.company.com --bastion-user admin
```

#### **2. Add a new connection**
```bash
# Basic connection (uses default bastion if configured)
bayesian-ssh add "My Server" server.company.com

# Complete connection with bastion
bayesian-ssh add "Prod Server" prod.company.com \
  --user admin \
  --port 22 \
  --kerberos \
  --bastion bastion.company.com \
  --bastion-user admin \
  --key ~/.ssh/id_rsa \
  --tags production,web

# Direct EC2 instance (no bastion)
bayesian-ssh add "Web EC2" ec2-54-123-45-67.compute-1.amazonaws.com \
  --user ubuntu \
  --kerberos false \
  --key ~/.ssh/ec2-key.pem \
  --no-bastion \
  --tags ec2,production,web
```

#### **3. Manage connections**
```bash
# List all connections
bayesian-ssh list

# Detailed view
bayesian-ssh list --detailed

# Filter by tag
bayesian-ssh list --tag production

# Recent connections
bayesian-ssh list --recent

# View connection details
bayesian-ssh show "My Server"

# Edit a connection
bayesian-ssh edit "My Server" --add-tags critical --remove-tags old

# Remove a connection
bayesian-ssh remove "My Server"
```

#### **4. Application configuration**
```bash
# View current configuration
bayesian-ssh config

# Modify configuration
bayesian-ssh config \
  --default-user admin \
  --default-bastion bastion.company.com \
  --use-kerberos

# Usage statistics
bayesian-ssh stats
```

#### **5. Import and migration**
```bash
# Import from SSH config
bayesian-ssh import

# Import from a specific file
bayesian-ssh import --file /path/to/ssh/config
```

## 🔧 Configuration

### 📁 **Configuration files**
The application automatically creates its configuration in `~/.config/bayesian-ssh/`:

```
~/.config/bayesian-ssh/
├── config.json          # Application configuration
└── history.db           # SQLite database
```

### ⚙️ **Configurable parameters**
```json
{
  "database_path": "~/.config/bayesian-ssh/history.db",
  "default_user": "admin",
  "default_bastion": "bastion-server.company.priv",
  "default_bastion_user": "admin",
  "default_port": 22,
  "use_kerberos_by_default": true,
  "log_level": "info",
  "auto_save_history": true,
  "max_history_size": 1000
}
```

### 🔐 **Kerberos configuration**
```bash
# Enable Kerberos by default
bayesian-ssh config --use-kerberos

# Configure default realm (if needed)
export KRB5_CONFIG=/etc/krb5.conf
```

## 🏗️ Technical Architecture

Bayesian SSH is built with a modular architecture that separates concerns and provides a clean, maintainable codebase. For detailed technical information, see the [Technical Architecture](docs/technical-architecture.md) documentation.

**Key Components:**
- **CLI Layer** - Command parsing and shell completions
- **Configuration Management** - JSON-based settings
- **Data Models** - Connection and session structures
- **Database Layer** - SQLite persistence with efficient queries
- **Services** - SSH and Kerberos integration

## 📊 **Advanced Usage Examples**

For comprehensive examples including enterprise environments, cloud infrastructure, Kubernetes, multi-cloud setups, and more, see the [Advanced Usage](docs/advanced-usage.md) documentation.

**Quick Examples:**
```bash
# Enterprise with bastion
bayesian-ssh config --default-bastion bastion.company.com --use-kerberos
bayesian-ssh add "Web Prod" web-prod.company.com --tags production,web

# Cloud instances (direct connection)
bayesian-ssh add "EC2 Web" ec2-web.company.com --user ubuntu --kerberos false --no-bastion

# Import existing SSH config
bayesian-ssh import
```

**Bastion Management:**
```bash
# Use default bastion (from config)
bayesian-ssh add "Server" host.com

# Force direct connection
bayesian-ssh add "Server" host.com --no-bastion

# Use specific bastion
bayesian-ssh add "Server" host.com --bastion custom-bastion.com
```

## 🐛 **Troubleshooting**

For comprehensive troubleshooting including Kerberos issues, SSH problems, database errors, performance issues, and more, see the [Troubleshooting Guide](docs/troubleshooting.md).

**Quick Fixes:**
```bash
# Kerberos ticket issues
klist -s && kinit -f -A

# SSH permission problems
chmod 600 ~/.ssh/id_rsa

# Database issues
rm ~/.config/bayesian-ssh/history.db && bayesian-ssh stats

# Enable debug logging
bayesian-ssh config --log-level debug
```

## 🔮 **Roadmap and Future Features**

For detailed development plans, feature timelines, and contribution guidelines, see the [Roadmap](docs/roadmap.md) documentation.

**Upcoming Features:**
- 🎨 **Tauri GUI Interface** - Modern React-based interface
- ☁️ **Cloud Integrations** - AWS, Azure, GCP, Kubernetes
- 🔐 **Advanced Security** - Vault integration, MFA, encryption
- 🚀 **Advanced Features** - Scripts, connection pooling, API

## 🤝 **Contribution**

### 📋 **How to contribute**
1. **Fork** the project
2. **Create** a feature branch (`git checkout -b feature/AmazingFeature`)
3. **Commit** your changes (`git commit -m 'Add AmazingFeature'`)
4. **Push** to the branch (`git push origin feature/AmazingFeature`)
5. **Open** a Pull Request

### 🐛 **Report a bug**
- Use the **Issues** tab on GitHub
- Describe the problem in detail
- Include logs and configuration
- Specify your environment

### 💡 **Propose a feature**
- Open an **Issue** with the "enhancement" label
- Describe the use case
- Explain the benefits
- Propose an implementation

## 📄 **License**

This project is licensed under **MIT**. See the [LICENSE](LICENSE) file for more details.

## 🙏 **Acknowledgments**

- **Rust** : For performance and security
- **Tauri** : For modern architecture
- **SQLite** : For data persistence
- **The open source community** : For inspiration

---

## 📚 **Documentation**

For comprehensive documentation, examples, and advanced topics, visit the [docs/](docs/) folder:

- **[Technical Architecture](docs/technical-architecture.md)** - System design and architecture
- **[Advanced Usage](docs/advanced-usage.md)** - Complex scenarios and enterprise use cases  
- **[Troubleshooting](docs/troubleshooting.md)** - Common issues and solutions
- **[Roadmap](docs/roadmap.md)** - Future features and development plans
- **[Documentation Index](docs/README.md)** - Complete documentation overview

## 🚀 **Ready to get started?**

```bash
# Clone and compile
git clone https://github.com/yourusername/bayesian-ssh.git
cd bayesian-ssh
cargo build --release

# First connection
./target/release/bayesian-ssh add "My Server" server.company.com
./target/release/bayesian-ssh connect "My Server"
```

**Transform your SSH experience today!** 🎯

---

*Developed with ❤️ by the Abdoufermat and Cursor*

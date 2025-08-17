# 🚀 Bayesian SSH - Intelligent SSH Session Manager

[![Rust](https://img.shields.io/badge/Rust-1.70+-blue.svg)](https://rustup.rs/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![CI](https://github.com/abdoufermat5/bayesian-ssh/workflows/CI/badge.svg)](https://github.com/abdoufermat5/bayesian-ssh/actions/workflows/ci.yml)

> **An ultra-fast and intelligent SSH session manager with Kerberos support, bastion hosts, and advanced history management.**

## 🎯 What is Bayesian SSH?

**Bayesian SSH** transforms your SSH experience with intelligent automation:

- 🚀 **One-click connections** to your servers
- 🔐 **Automatic Kerberos** ticket management
- 🚪 **Smart bastion host** routing
- 🏷️ **Tag-based organization** for easy management
- 📊 **Complete connection history** with statistics
- 💾 **SQLite database** for persistence

## 🚀 Quick Start

### Installation
```bash
# Clone and build
git clone https://github.com/abdoufermat5/bayesian-ssh.git
cd bayesian-ssh

# Build and install
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
# Connect to a server
bayesian-ssh connect "Server Name"

# Add new connection
bayesian-ssh add "Server Name" hostname.com

# List connections
bayesian-ssh list

# Show details
bayesian-ssh show "Server Name"

# Import from SSH config
bayesian-ssh import
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

## 🔧 Development & Release

### Build Scripts
```bash
# Build and test
./scripts/build_and_push.sh

# Build release version
./scripts/build_and_push.sh --release

# Update version and build
./scripts/build_and_push.sh --version X.Y.Z

# Create release (build, tag, and push tag)
./scripts/build_and_push.sh --release --create-release

# Full release workflow with version update
./scripts/build_and_push.sh --version X.Y.Z --release --create-release

# Clean all tags and releases
./scripts/clean_releases.sh
```

### Configuration

The app automatically creates configuration in `~/.config/bayesian-ssh/`:

```bash
# View current config
bayesian-ssh config

# Set defaults
bayesian-ssh config --default-user admin --use-kerberos
```

## 📚 Documentation

For comprehensive guides and advanced topics, see the [docs/](docs/) folder:

- **[Technical Architecture](docs/technical-architecture.md)** - System design and architecture
- **[Advanced Usage](docs/advanced-usage.md)** - Enterprise scenarios and complex use cases  
- **[Troubleshooting](docs/troubleshooting.md)** - Common issues and solutions
- **[Documentation Index](docs/README.md)** - Complete documentation overview

## 🆕 Recent Updates

### v0.1.0 (Current)
- ✅ **Core CLI functionality** with connection management
- ✅ **Kerberos integration** with automatic ticket handling
- ✅ **Bastion host support** with smart routing
- ✅ **SSH config import** for easy migration
- ✅ **Tag-based organization** for efficient management
- ✅ **CI pipeline** with automated testing
- ✅ **Comprehensive documentation** with examples

### Coming Soon
- 🎨 **Tauri GUI Interface** - Modern React-based interface
- ☁️ **Cloud Integrations** - AWS, Azure, GCP, Kubernetes
- 🔐 **Advanced Security** - Vault integration, MFA support

## 🤝 Contributing

1. **Fork** the project
2. **Create** a feature branch (`git checkout -b feature/AmazingFeature`)
3. **Commit** your changes (`git commit -m 'Add AmazingFeature'`)
4. **Push** to the branch (`git push origin feature/AmazingFeature`)
5. **Open** a Pull Request

## 📄 License

This project is licensed under **MIT**. See the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- **Rust** : For performance and security
- **SQLite** : For data persistence
- **The open source community** : For inspiration

---

**Transform your SSH experience today!** 🎯

*Developed with ❤️ by Abdoufermat*

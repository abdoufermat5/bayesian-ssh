# Bayesian SSH Documentation

Welcome to the comprehensive documentation for Bayesian SSH, the intelligent SSH session manager built with Rust.

## Documentation Structure

### Getting Started
- **[Main README](../README.md)** - Quick start guide and basic usage
- **[Installation Guide](#installation)** - Setup and configuration
- **[Basic Usage](#basic-usage)** - Essential commands and examples

### Core Documentation
- **[Technical Architecture](technical-architecture.md)** - System design and architecture
- **[Advanced Usage](advanced-usage.md)** - Complex scenarios and enterprise use cases
- **[Troubleshooting](troubleshooting.md)** - Common issues and solutions
- **[Roadmap](roadmap.md)** - Future features and development plans

### Reference
- **[Command Reference](#command-reference)** - Complete CLI command documentation
- **[Configuration Reference](#configuration)** - All configuration options
- **[API Reference](#api-reference)** - Programmatic interface documentation

## Quick Start

### Installation
```bash
# Clone and build
git clone https://github.com/yourusername/bayesian-ssh.git
cd bayesian-ssh
cargo build --release

# Install globally (optional)
sudo cp target/release/bayesian-ssh /usr/local/bin/
```

### First Connection
```bash
# Add a server
bayesian-ssh add "My Server" server.company.com

# Connect to it
bayesian-ssh connect "My Server"
```

### Enable Tab Completion
```bash
# Generate completion script
./target/release/bayesian-ssh completions bash > bayesian-ssh-completion.bash

# Source it
source bayesian-ssh-completion.bash
```

## Basic Commands

### Connection Management
```bash
# Add a new connection
bayesian-ssh add "Server Name" hostname.com

# Add with specific bastion
bayesian-ssh add "Server Name" hostname.com --bastion bastion.company.com

# Force direct connection (no bastion)
bayesian-ssh add "Server Name" hostname.com --no-bastion

# List all connections
bayesian-ssh list

# Connect to a server
bayesian-ssh connect "Server Name"

# Connect with custom parameters
bayesian-ssh connect "Server Name" --no-bastion --user customuser

# Show connection details
bayesian-ssh show "Server Name"

# Edit a connection
bayesian-ssh edit "Server Name"

# Remove a connection
bayesian-ssh remove "Server Name"
```

### Configuration
```bash
# View current configuration
bayesian-ssh config

# Update configuration (Kerberos disabled by default)
bayesian-ssh config --use-kerberos --default-user customuser

# Set default bastion
bayesian-ssh config --default-bastion bastion.company.com

# View statistics
bayesian-ssh stats
```

### Import/Export
```bash
# Import from SSH config
bayesian-ssh import

# Import from specific file
bayesian-ssh import --file /path/to/ssh/config
```

## Configuration

### Configuration File Location
```
~/.config/bayesian-ssh/
├── config.json          # Application configuration
└── history.db           # SQLite database
```

### Key Configuration Options
```json
{
  "default_user": "current-system-user",
  "default_bastion": "bastion.company.com",
  "default_bastion_user": "current-system-user",
  "use_kerberos_by_default": false,
  "log_level": "info",
  "auto_save_history": true,
  "max_history_size": 1000
}
```

## Use Cases

### Enterprise Environment
- Bastion host management - Secure jump host connections
- Kerberos authentication - Automatic ticket management
- Tag-based organization - Production, staging, development
- Centralized management - Single source of truth for connections

### Cloud Infrastructure
- AWS EC2 instances - Direct and bastion connections
- Multi-cloud setup - AWS, Azure, GCP support
- Kubernetes access - Pod and service connections
- Container management - Docker and container access

### Development Workflow
- Environment management - Dev, staging, production
- CI/CD integration - Automated deployment access
- Feature development - Isolated development environments
- Testing and QA - Dedicated testing infrastructure

## Security Features

### Authentication
- Kerberos integration - Enterprise authentication
- SSH key management - Secure key handling
- Bastion support - Secure jump host connections
- Credential encryption - Secure storage of sensitive data

### Access Control
- User permissions - Role-based access control
- Audit logging - Complete connection history
- Policy enforcement - Security policy management
- Compliance support - SOX, PCI, HIPAA compliance

## Performance and Monitoring

### Connection Metrics
- Response times - Connection establishment speed
- Success rates - Connection reliability metrics
- Usage patterns - Peak usage and frequency analysis
- Health monitoring - Server availability tracking

### Optimization
- Connection pooling - Efficient connection reuse
- Load balancing - Intelligent server selection
- Caching - Fast connection lookup
- Async operations - Non-blocking I/O operations

## Getting Help

### Documentation
- This guide - Comprehensive documentation
- Examples - Real-world usage examples
- Troubleshooting - Common issues and solutions
- API reference - Programmatic interface

### Community Support
- GitHub Issues - Bug reports and feature requests
- Discussions - Community Q&A and support
- Contributing - How to contribute to the project
- Code of Conduct - Community guidelines

### Professional Support
- Enterprise support - Commercial support options
- Training - User and administrator training
- Consulting - Implementation and optimization services
- Custom development - Tailored feature development

## Development

### Building from Source
```bash
# Prerequisites
rustup install stable
cargo install --version 0.29.0 sqlx-cli

# Build
cargo build --release

# Run tests
cargo test

# Run with debug logging
RUST_LOG=debug cargo run
```

### Contributing
1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests and documentation
5. Submit a pull request

### Development Guidelines
- Rust best practices - Follow Rust coding standards
- Testing - Maintain high test coverage
- Documentation - Document all public APIs
- Performance - Optimize for speed and efficiency

## License

Bayesian SSH is licensed under the MIT License. See the [LICENSE](../LICENSE) file for details.

## Acknowledgments

- Rust community - For the amazing language and ecosystem
- Open source contributors - For their valuable contributions
- Users and testers - For feedback and bug reports
- Enterprise users - For real-world testing and validation

---

**Need help?** Check the [troubleshooting guide](troubleshooting.md) or [open an issue](https://github.com/yourusername/bayesian-ssh/issues) on GitHub.

**Want to contribute?** See our [contributing guidelines](../CONTRIBUTING.md) and [roadmap](roadmap.md).

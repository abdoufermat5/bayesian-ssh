# Installation

## Option 1: One-liner Install (Recommended)

```bash
# Install latest release automatically (non-interactive)
curl -fsSL https://raw.githubusercontent.com/abdoufermat5/bayesian-ssh/main/install.sh | bash
```

```bash
# Interactive installation (choose options)
curl -fsSL https://raw.githubusercontent.com/abdoufermat5/bayesian-ssh/main/install.sh | bash -s -- --interactive
```

## Option 2: Manual Build

### Prerequisites

```bash
rustup install stable
```

### Build and Install

```bash
# Clone and build
git clone https://github.com/abdoufermat5/bayesian-ssh.git
cd bayesian-ssh

# Build and install using Makefile
make release
make install

# Or build manually
cargo build --release
sudo cp target/release/bayesian-ssh /usr/local/bin/
```

## Verify Installation

```bash
bayesian-ssh --version
```

## Enable Tab Completion

Generate and source a completion script for your shell:

```bash
# Bash
bayesian-ssh completions bash > bayesian-ssh-completion.bash
source bayesian-ssh-completion.bash

# Zsh
bayesian-ssh completions zsh > _bayesian-ssh
# Move to your zsh completions directory

# Fish
bayesian-ssh completions fish > bayesian-ssh.fish
# Move to your fish completions directory
```

To make completions permanent, add the `source` line to your shell's rc file (e.g. `~/.bashrc`).

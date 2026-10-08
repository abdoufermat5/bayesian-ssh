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

## Option 3: Snap Store

```bash
sudo snap install bayesian-ssh
sudo snap connect bayesian-ssh:ssh-keys   # read-only ~/.ssh and /etc/ssh
sudo snap alias bayesian-ssh bssh         # optional short alias
```

The snap ships both the CLI (`bayesian-ssh`, or `bssh` after the alias) and the
desktop app (`bayesian-ssh.gui`). Strict confinement limits it — `~/.ssh` is
read-only (`known_hosts` is not updated, `key generate` cannot write there), its
config/database live under `~/snap/bayesian-ssh/current/.config/bayesian-ssh`,
it uses its own `ssh`, and it has no Kerberos support (Kerberos is optional; use
a native install if you need it). See
[Distribution](../reference/distribution.md#snap-store) for the full list and how
to publish.

## Verify Installation

```bash
bayesian-ssh --version
```

### Verify a release

Release assets are signed. With [`cosign`](https://docs.sigstore.dev/cosign/)
and the [`gh`](https://cli.github.com/) CLI installed, check a downloaded
release like this:

```bash
cosign verify-blob --bundle SHA256SUMS.sigstore.json \
  --certificate-identity-regexp '^https://github\.com/abdoufermat5/bayesian-ssh/\.github/workflows/release\.yml@refs/(tags/v.+|heads/main)$' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com SHA256SUMS

sha256sum --ignore-missing -c SHA256SUMS

gh attestation verify bayesian-ssh-linux-x86_64.tar.gz --repo abdoufermat5/bayesian-ssh
```

`install.sh` performs the `cosign` check automatically when `cosign` is on
`PATH`, and always verifies `SHA256SUMS` with `sha256sum -c`. See
[Distribution](../reference/distribution.md#release-signing) for details.

### In-app updates

The desktop `.AppImage` and the `bayesian-ssh-desktop` `.deb`/`.rpm` from the
GitHub release can update themselves from **Settings → Updates**; installing
closes open sessions and restarts the app. The snap, the `install.sh` binaries,
the unified `bayesian-ssh` packages and source builds are updated the way they
were installed.

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

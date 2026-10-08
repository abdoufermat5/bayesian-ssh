# Contributing to Bayesian SSH

Thank you for your interest in contributing to **Bayesian SSH**!

## 🚀 Quick Setup

### Prerequisites
- **Rust** 1.70+ (`rustup update`)
- **Node.js** 18+ (for Desktop GUI)
- **C/C++ Build Tools** (`build-essential`, `pkg-config`, `libssl-dev`)

### Development Workflow

```bash
# Clone the repository
git clone https://github.com/abdoufermat5/bayesian-ssh.git
cd bayesian-ssh

# Build the frontend, then debug CLI & desktop GUI binaries
make build

# Run the desktop GUI with hot reload (Vite HMR on port 1420; Rust changes
# rebuild and restart the app automatically)
make gui-dev

# Run unit and integration tests for the whole workspace
make test

# Run the desktop end-to-end tests (Playwright; needs the frontend dependencies)
make e2e

# Format code and run linter (clippy, whole workspace, warnings are errors)
make format
make lint

# Same formatting check CI runs
cargo fmt --all -- --check
```

The GUI crate embeds `desktop/build` at compile time, so `make build`, `make test`,
`make check` and `make lint` build the frontend first (requires `npm`).

`make gui-dev` runs the Vite dev server on port 1420 and starts the app with
`--no-default-features`, which drops the `custom-protocol` feature so the
window loads `build.devUrl` instead of the prebuilt assets. Frontend edits
hot-reload; Rust edits need a restart. Close the installed app first — the
single-instance plugin makes a second instance exit.

The Tauri CLI is not used for development: it resolves the app's Cargo package
by comparing the literal `src-tauri/Cargo.toml` path against cargo's canonical
manifest path, which never matches the `desktop/src-tauri` symlink.

## 📦 Packaging & Builds

```bash
# Build unified .deb and .rpm packages
make package

# Build only the desktop frontend
make frontend
```

## 📜 Pull Request Guidelines

1. **Keep PRs focused**: One feature or bugfix per PR.
2. **Format code**: Run `make format` before committing.
3. **Verify tests**: Ensure `make test` and `make lint` pass without warnings.
4. **License**: By contributing, you agree that your contributions will be licensed under the project's [MIT License](LICENSE).

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

# Check & build debug version
make build

# Run unit and integration tests
make test

# Format code and run linter
make format
make lint

# Run pre-commit checks
make pre-commit
```

## 📦 Packaging & Builds

```bash
# Build unified .deb and .rpm packages
make package

# Build Desktop GUI
make build-desktop
```

## 📜 Pull Request Guidelines

1. **Keep PRs focused**: One feature or bugfix per PR.
2. **Format code**: Run `make format` before committing.
3. **Verify tests**: Ensure `make test` and `make lint` pass without warnings.
4. **License**: By contributing, you agree that your contributions will be licensed under the project's [MIT License](LICENSE).

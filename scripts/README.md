# Scripts

This directory contains utility scripts for building, releasing, and managing the Bayesian SSH project.

## build_and_push.sh

**Purpose**: Build, test, and create releases for Bayesian SSH.

### Basic Usage
```bash
# Build and test
./scripts/build_and_push.sh

# Build release version
./scripts/build_and_push.sh --release

# Update version and build
./scripts/build_and_push.sh --version 1.2.3

# Create release (build, tag, push tag)
./scripts/build_and_push.sh --release --create-release

# Full workflow: update version, build, create release
./scripts/build_and_push.sh --version 1.2.3 --release --create-release
```

### What It Does
- Builds the project (debug or release)
- Runs tests and formatting checks
- Updates version numbers across all files
- Creates git tags for releases
- Pushes tags to remote repository

## clean_releases.sh

**Purpose**: Clean up tags and releases both locally and remotely.

### Basic Usage
```bash
# Clean everything (with confirmation)
./scripts/clean_releases.sh

# Delete specific tag
./scripts/clean_releases.sh --tag v1.2.3

# Clean only remote tags
./scripts/clean_releases.sh --remote-only

# Clean only local tags
./scripts/clean_releases.sh --local-only

# Skip confirmation prompt
./scripts/clean_releases.sh --force
```

### What It Does
- Removes local and remote git tags
- Cleans up git references
- Offers selective cleanup options
- Safe deletion with confirmation prompts

## Quick Workflow

1. **Clean old releases**: `./scripts/clean_releases.sh`
2. **Update version**: `./scripts/build_and_push.sh --version 1.2.3`
3. **Create release**: `./scripts/build_and_push.sh --release --create-release`
4. **Install**: `sudo cp target/release/bayesian-ssh /usr/local/bin/`

## Notes

- Both scripts require a git repository
- Release creation works best from the `main` branch
- Version format must be `X.Y.Z` (e.g., `1.2.3`)
- Scripts handle file updates automatically

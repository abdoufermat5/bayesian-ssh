.PHONY: help frontend gui-dev build release test check format lint e2e install uninstall package flatpak-build snap-build docs clean

INSTALL_DIR ?= /usr/local/bin

# snap-build: release tag to repackage and target architecture.
TAG ?=
ARCH ?= amd64

help: ## Show this help message
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'

# The GUI crate embeds desktop/build at compile time, so every workspace-wide
# cargo command needs the frontend built first.
frontend: ## Build the desktop frontend (desktop/build)
	@if [ -d "desktop" ]; then \
		export PATH=$$PATH:$$HOME/.nvm/versions/node/$$(ls $$HOME/.nvm/versions/node 2>/dev/null | sort -V | tail -n 1)/bin:$$HOME/.cargo/bin; \
		if command -v npm >/dev/null 2>&1; then cd desktop && ( [ -d "node_modules" ] || npm ci ) && npm run build; fi; \
	fi

build: frontend ## Build debug CLI & GUI binaries
	cargo build --workspace

# Hot-reload development run. The Vite dev server serves the frontend on port
# 1420 and the app is built by scripts/gui-dev.sh; see that script for why the
# Tauri CLI is not used here.
gui-dev: ## Run the desktop GUI with hot reload (Vite HMR; restart for Rust changes)
	@./scripts/gui-dev.sh

release: frontend ## Build release binaries (CLI + Desktop GUI)
	cargo build --release --workspace

test: frontend ## Run tests for the whole workspace
	cargo test --workspace

check: frontend ## Check code compilation
	cargo check --workspace --all-targets

format: ## Format Rust code
	cargo fmt --all

lint: frontend ## Run Clippy linter on the whole workspace
	cargo clippy --workspace --all-targets -- -D warnings

# End-to-end tests for the desktop GUI (Playwright + chromium). Starts/reuses
# the Vite dev server on port 1420 and stubs the Tauri IPC layer.
e2e: ## Run the desktop end-to-end test suite (Playwright)
	@if [ -d "desktop" ]; then \
		export PATH=$$PATH:$$HOME/.nvm/versions/node/$$(ls $$HOME/.nvm/versions/node 2>/dev/null | sort -V | tail -n 1)/bin:$$HOME/.cargo/bin; \
		if command -v npm >/dev/null 2>&1; then cd desktop && ( [ -d "node_modules" ] || npm ci ) && npm run e2e; fi; \
	fi

install: release ## Install bayesian-ssh, bssh alias, and desktop GUI binary to system
	sudo install -m 755 target/release/bayesian-ssh "$(INSTALL_DIR)/bayesian-ssh"
	sudo ln -sf "$(INSTALL_DIR)/bayesian-ssh" "$(INSTALL_DIR)/bssh"
	@if [ -f "target/release/bayesian-ssh-gui" ]; then \
		sudo install -m 755 target/release/bayesian-ssh-gui "$(INSTALL_DIR)/bayesian-ssh-gui"; \
	elif [ -f "target/release/bayesian-ssh-desktop" ]; then \
		sudo install -m 755 target/release/bayesian-ssh-desktop "$(INSTALL_DIR)/bayesian-ssh-gui"; \
	fi

uninstall: ## Remove bayesian-ssh, bssh, and desktop GUI binary from system
	sudo rm -f $(INSTALL_DIR)/bayesian-ssh $(INSTALL_DIR)/bssh $(INSTALL_DIR)/bayesian-ssh-gui

package: ## Build unified .deb and .rpm packages
	./scripts/package.sh

flatpak-build: ## Build Flatpak bundle
	flatpak-builder --force-clean --ccache --install-deps-from=flathub target/flatpak-build packaging/flatpak/com.bayesianssh.App.yml

snap-build: ## Build Snap package from a published release (TAG=vX.Y.Z [ARCH=amd64|arm64])
	@if [ -z "$(TAG)" ]; then \
		echo "usage: make snap-build TAG=vX.Y.Z [ARCH=amd64|arm64]"; \
		exit 1; \
	fi
	scripts/snap-stage.sh $(TAG) $(ARCH) && cd target/snap && snapcraft pack

docs: ## Build documentation with mdBook
	mdbook build

clean: ## Clean build artifacts
	cargo clean
	rm -rf target/package-stage target/packages target/flatpak-build .flatpak-builder

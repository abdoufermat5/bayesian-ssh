.PHONY: help build release test check format lint install uninstall package flatpak-build snap-build docs clean

INSTALL_DIR ?= /usr/local/bin

help: ## Show this help message
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'

build: ## Build debug CLI & GUI binaries
	@if [ -d "desktop" ] && command -v npm >/dev/null 2>&1; then cd desktop && ( [ -d "node_modules" ] || npm install ) && npm run build; fi
	cargo build --workspace

release: ## Build release binaries (CLI + Desktop GUI)
	cargo build --release --workspace
	@if [ -d "desktop" ] && command -v npm >/dev/null 2>&1; then \
		cd desktop && ( [ -d "node_modules" ] || npm install ) && npm run tauri build -- --config ../crates/gui/tauri.conf.json --no-bundle; \
	fi

test: ## Run tests
	cargo test

check: ## Check code compilation
	cargo check

format: ## Format Rust code
	cargo fmt --all

lint: ## Run Clippy linter
	cargo clippy -- -D warnings

install: release ## Install bayesian-ssh, bssh alias, and desktop GUI binary to system
	sudo install -m 755 target/release/bayesian-ssh $(INSTALL_DIR)/bayesian-ssh
	sudo ln -sf $(INSTALL_DIR)/bayesian-ssh $(INSTALL_DIR)/bssh
	@if [ -f "target/release/bayesian-ssh-gui" ]; then \
		sudo install -m 755 target/release/bayesian-ssh-gui $(INSTALL_DIR)/bayesian-ssh-gui; \
	fi

uninstall: ## Remove bayesian-ssh, bssh, and desktop GUI binary from system
	sudo rm -f $(INSTALL_DIR)/bayesian-ssh $(INSTALL_DIR)/bssh $(INSTALL_DIR)/bayesian-ssh-gui

package: ## Build unified .deb and .rpm packages
	./scripts/package.sh

flatpak-build: ## Build Flatpak bundle
	flatpak-builder --force-clean --install-deps-from=flathub target/flatpak-build packaging/flatpak/com.bayesianssh.App.yml

snap-build: ## Build Snap package
	snapcraft --manifest=packaging/snap/snapcraft.yaml

docs: ## Build documentation with mdBook
	mdbook build

clean: ## Clean build artifacts
	cargo clean
	rm -rf target/package-stage target/packages target/flatpak-build .flatpak-builder

.PHONY: help build release test check format lint install uninstall package flatpak-build snap-build docs clean

INSTALL_DIR ?= /usr/local/bin

help: ## Show this help message
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'

build: ## Build debug CLI version
	cargo build

release: ## Build release CLI version
	cargo build --release

test: ## Run tests
	cargo test

check: ## Check code compilation
	cargo check

format: ## Format Rust code
	cargo fmt --all

lint: ## Run Clippy linter
	cargo clippy -- -D warnings

install: release ## Install bayesian-ssh and bssh alias to system
	sudo install -m 755 target/release/bayesian-ssh $(INSTALL_DIR)/bayesian-ssh
	sudo ln -sf $(INSTALL_DIR)/bayesian-ssh $(INSTALL_DIR)/bssh

uninstall: ## Remove bayesian-ssh and bssh from system
	sudo rm -f $(INSTALL_DIR)/bayesian-ssh $(INSTALL_DIR)/bssh

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

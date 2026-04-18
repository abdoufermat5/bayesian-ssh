# Bayesian SSH Makefile
# Provides common development and build tasks

.PHONY: help build test release clean install uninstall format lint check deps update-deps docs docs-serve docs-api demo-up demo-test demo-down demo-ssh-bastion demo-ssh-target1 demo-ssh-target2

# Configuration
BINARY_NAME = bayesian-ssh
CARGO = cargo
RUSTUP = rustup
TARGET_DIR = target
RELEASE_DIR = $(TARGET_DIR)/release
INSTALL_DIR = /usr/local/bin

# Default target
help: ## Show this help message
	@echo "🚀 Bayesian SSH - Available Commands"
	@echo "====================================="
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "📚 Examples:"
	@echo "  make build          # Build debug version"
	@echo "  make release        # Build release version"
	@echo "  make test           # Run all tests"
	@echo "  make install        # Install to system"
	@echo "  make clean          # Clean build artifacts"

# Build targets
build: ## Build debug version
	@echo "🔨 Building debug version..."
	$(CARGO) build
	@echo "✅ Build completed"

release: ## Build release version
	@echo "🔨 Building release version..."
	$(CARGO) build --release
	@echo "✅ Release build completed"
	@echo "📊 Binary size: $(shell ls -lh $(RELEASE_DIR)/$(BINARY_NAME) 2>/dev/null | awk '{print $$5}' || echo "N/A")"

# Testing and quality
test: ## Run all tests
	@echo "🧪 Running tests..."
	$(CARGO) test
	@echo "✅ Tests completed"

check: ## Run cargo check
	@echo "🔍 Running cargo check..."
	$(CARGO) check
	@echo "✅ Check completed"

format: ## Format code with rustfmt
	@echo "🎨 Formatting code..."
	$(CARGO) fmt --all
	@echo "✅ Formatting completed"

format-check: ## Check code formatting
	@echo "🎨 Checking code formatting..."
	$(CARGO) fmt --all -- --check
	@echo "✅ Formatting check completed"

lint: ## Run clippy linter
	@echo "🔍 Running clippy..."
	$(CARGO) clippy -- -D warnings
	@echo "✅ Linting completed"

# Dependencies
deps: ## Install development dependencies
	@echo "📦 Installing development dependencies..."
	$(RUSTUP) component add rustfmt
	$(RUSTUP) component add clippy
	@echo "✅ Dependencies installed"

update-deps: ## Update Rust toolchain and dependencies
	@echo "🔄 Updating Rust toolchain..."
	$(RUSTUP) update
	@echo "🔄 Updating Cargo dependencies..."
	$(CARGO) update
	@echo "✅ Updates completed"

# Installation
install: release ## Install binary to system
	@echo "📦 Installing $(BINARY_NAME)..."
	@if [ -f "$(RELEASE_DIR)/$(BINARY_NAME)" ]; then \
		sudo cp "$(RELEASE_DIR)/$(BINARY_NAME)" "$(INSTALL_DIR)/$(BINARY_NAME)"; \
		echo "✅ $(BINARY_NAME) installed to $(INSTALL_DIR)"; \
	else \
		echo "❌ Release binary not found. Run 'make release' first."; \
		exit 1; \
	fi

uninstall: ## Remove binary from system
	@echo "🗑️  Uninstalling $(BINARY_NAME)..."
	@if [ -f "$(INSTALL_DIR)/$(BINARY_NAME)" ]; then \
		sudo rm "$(INSTALL_DIR)/$(BINARY_NAME)"; \
		echo "✅ $(BINARY_NAME) uninstalled from $(INSTALL_DIR)"; \
	else \
		echo "ℹ️  $(BINARY_NAME) not found in $(INSTALL_DIR)"; \
	fi

# Development workflow
dev: format lint test ## Run full development workflow
	@echo "🎉 Development workflow completed!"

pre-commit: format lint test ## Run before committing
	@echo "🎉 Pre-commit checks passed!"

# Cleanup
clean: ## Clean build artifacts
	@echo "🧹 Cleaning build artifacts..."
	$(CARGO) clean
	@echo "✅ Cleanup completed"

distclean: clean ## Deep clean (removes target and Cargo.lock)
	@echo "🧹 Deep cleaning..."
	rm -rf $(TARGET_DIR)
	rm -f Cargo.lock
	@echo "✅ Deep cleanup completed"

# Release management
version: ## Show current version
	@echo "📋 Current version: $(shell grep '^version = ' Cargo.toml | cut -d'"' -f2)"

bump-patch: ## Bump patch version
	@echo "🔄 Bumping patch version..."
	@./scripts/build_and_push.sh --version $(shell ./scripts/build_and_push.sh --version | grep -o '[0-9]\+\.[0-9]\+\.[0-9]\+' | awk -F. '{print $$1"."$$2"."$$3+1}')
	@echo "✅ Version bumped"

bump-minor: ## Bump minor version
	@echo "🔄 Bumping minor version..."
	@./scripts/build_and_push.sh --version $(shell ./scripts/build_and_push.sh --version | grep -o '[0-9]\+\.[0-9]\+\.[0-9]\+' | awk -F. '{print $$1"."$$2+1".0"}')
	@echo "✅ Version bumped"

bump-major: ## Bump major version
	@echo "🔄 Bumping major version..."
	@./scripts/build_and_push.sh --version $(shell ./scripts/build_and_push.sh --version | grep -o '[0-9]\+\.[0-9]\+\.[0-9]\+' | awk -F. '{print $$1+1".0.0"}')
	@echo "✅ Version bumped"

# Quick shortcuts
all: clean deps build test release ## Full build pipeline
	@echo "🎉 Full build pipeline completed!"

# ── Demo Environment (Vagrant) ──────────────────────────────
demo-up: ## Boot the Vagrant demo VMs
	@echo "🚀 Starting demo VMs..."
	@cd demo && vagrant up

demo-test: build ## Run integration tests against demo VMs
	@echo "🧪 Running integration tests..."
	@cd demo && bash test-all.sh --skip-build

demo-down: ## Destroy the Vagrant demo VMs
	@echo "🗑️  Destroying demo VMs..."
	@cd demo && vagrant destroy -f

demo-ssh-bastion: ## SSH into the bastion VM
	@cd demo && vagrant ssh bastion

demo-ssh-target1: ## SSH into target1 VM
	@cd demo && vagrant ssh target1

demo-ssh-target2: ## SSH into target2 VM
	@cd demo && vagrant ssh target2

quick: build test ## Quick build and test
	@echo "🎉 Quick build and test completed!"

# Documentation
docs: ## Build documentation with mdBook
	@echo "📚 Building documentation..."
	mdbook build
	@echo "✅ Documentation built in docs/book/"

docs-serve: ## Serve documentation locally with live reload
	@echo "📚 Serving documentation..."
	mdbook serve --open

docs-api: ## Build Rust API documentation
	@echo "📚 Building API documentation..."
	$(CARGO) doc --no-deps --open
	@echo "✅ API documentation built"

# Show binary info
info: release ## Show binary information
	@echo "📊 Binary Information:"
	@echo "  Location: $(RELEASE_DIR)/$(BINARY_NAME)"
	@echo "  Size: $(shell ls -lh $(RELEASE_DIR)/$(BINARY_NAME) 2>/dev/null | awk '{print $$5}' || echo "N/A")"
	@echo "  Version: $(shell grep '^version = ' Cargo.toml | cut -d'"' -f2)"
	@echo "  Build time: $(shell date)"

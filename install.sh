#!/bin/bash

# Bayesian SSH Installer
# Automatically downloads and installs the latest release
# Usage: curl -fsSL https://raw.githubusercontent.com/abdoufermat5/bayesian-ssh/main/install.sh | bash
# Usage: curl -fsSL https://raw.githubusercontent.com/abdoufermat5/bayesian-ssh/main/install.sh | bash -s -- --interactive

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
REPO="abdoufermat5/bayesian-ssh"
BINARY_NAME="bayesian-ssh"
INSTALL_DIR="/usr/local/bin"
# Private, unpredictable work dir: a fixed /tmp path lets other local users
# pre-create it and swap the binary before it is installed with sudo.
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/bayesian-ssh-install.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT
INTERACTIVE=false
INSTALL_DESKTOP=false
NO_GUI=false

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --interactive)
            INTERACTIVE=true
            shift
            ;;
        --desktop)
            INSTALL_DESKTOP=true
            BINARY_NAME="bayesian-ssh-desktop"
            shift
            ;;
        --no-gui)
            NO_GUI=true
            INSTALL_DESKTOP=false
            shift
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--interactive] [--desktop] [--no-gui]"
            exit 1
            ;;
    esac
done

# Read a single-key answer from the terminal. When piped from curl, stdin is
# the script itself, so prompts must read from /dev/tty.
prompt() {
    REPLY=""
    read -p "$1" -n 1 -r < /dev/tty || true
    echo
}

# Check if script is being piped (non-interactive)
check_interactive() {
    if [ "$INTERACTIVE" = true ]; then
        echo -e "${BLUE}📋 Interactive mode enabled${NC}"
    elif [ -t 0 ]; then
        echo -e "${BLUE}📋 Interactive mode available (use --interactive flag)${NC}"
    else
        echo -e "${BLUE}📋 Non-interactive mode (piped from curl)${NC}"
    fi
}

# Detect system architecture and OS
detect_system() {
    echo -e "${BLUE}🔍 Detecting system...${NC}"
    
    # Architecture detection
    case "$(uname -m)" in
        x86_64|amd64)
            ARCH="x86_64"
            ;;
        aarch64|arm64)
            ARCH="aarch64"
            ;;
        armv7l|armv8l)
            ARCH="arm"
            ;;
        *)
            echo -e "${RED}❌ Unsupported architecture: $(uname -m)${NC}"
            exit 1
            ;;
    esac
    
    # OS detection
    case "$(uname -s)" in
        Linux)
            OS="linux"
            ;;
        Darwin)
            OS="macos"
            ;;
        *)
            echo -e "${RED}❌ Unsupported OS: $(uname -s)${NC}"
            exit 1
            ;;
    esac
    
    echo -e "${GREEN}✅ Detected: ${OS}-${ARCH}${NC}"
}

# Check if running as root
check_permissions() {
    if [ "$EUID" -eq 0 ]; then
        echo -e "${YELLOW}⚠️  Running as root - this is not recommended${NC}"
        if [ "$INTERACTIVE" = true ]; then
            prompt "Continue anyway? (y/N): "
            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                echo -e "${RED}❌ Installation cancelled${NC}"
                exit 1
            fi
        else
            echo -e "${YELLOW}⚠️  Continuing as root in non-interactive mode${NC}"
        fi
    fi
}

# Check dependencies
check_dependencies() {
    echo -e "${BLUE}🔍 Checking dependencies...${NC}"
    
    # Check for curl
    if ! command -v curl &> /dev/null; then
        echo -e "${RED}❌ curl is required but not installed${NC}"
        echo -e "${YELLOW}Please install curl and try again${NC}"
        exit 1
    fi

    # Check for a SHA-256 tool (downloads are verified against SHA256SUMS)
    if ! command -v sha256sum &> /dev/null && ! command -v shasum &> /dev/null; then
        echo -e "${RED}❌ sha256sum (or shasum) is required but not installed${NC}"
        exit 1
    fi
    
    echo -e "${GREEN}✅ Dependencies satisfied${NC}"
}

# Get latest release info
get_latest_release() {
    echo -e "${BLUE}📡 Fetching latest release...${NC}"
    
    # Try to get latest release using GitHub API
    if command -v jq &> /dev/null; then
        # Use jq if available for better parsing
        LATEST_TAG=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | jq -r '.tag_name')
    else
        # Fallback: /releases/latest redirects to /releases/tag/<latest tag>
        LATEST_TAG=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest")
        LATEST_TAG="${LATEST_TAG##*/tag/}"
    fi

    # The tag is interpolated into download URLs; accept only plain version tags.
    if [[ ! "$LATEST_TAG" =~ ^v[0-9][0-9A-Za-z.+-]*$ ]]; then
        echo -e "${RED}❌ Failed to get latest release${NC}"
        exit 1
    fi
    
    echo -e "${GREEN}✅ Latest release: ${LATEST_TAG}${NC}"
}

# Print the SHA-256 digest of a file
sha256_of() {
    if command -v sha256sum &> /dev/null; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

# Download binary
download_binary() {
    echo -e "${BLUE}📥 Downloading binary...${NC}"
    
    cd "$TEMP_DIR"
    
    # Construct download URL
    if [ "$INSTALL_DESKTOP" = true ]; then
        ASSET_NAME="bayesian-ssh-desktop-${OS}-${ARCH}"
    else
        ASSET_NAME="bayesian-ssh-${OS}-${ARCH}"
    fi
    RELEASE_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}"
    DOWNLOAD_URL="${RELEASE_URL}/${ASSET_NAME}"
    
    echo -e "${BLUE}📡 Downloading from: ${DOWNLOAD_URL}${NC}"
    
    if ! curl -fsSL -o "$BINARY_NAME" "$DOWNLOAD_URL"; then
        echo -e "${RED}❌ Download failed: no ${ASSET_NAME} asset in release ${LATEST_TAG}${NC}"
        echo -e "${YELLOW}Please check the release page manually: https://github.com/${REPO}/releases${NC}"
        exit 1
    fi

    echo -e "${BLUE}🔒 Verifying checksum...${NC}"
    if ! curl -fsSL -o SHA256SUMS "${RELEASE_URL}/SHA256SUMS"; then
        echo -e "${RED}❌ Could not download SHA256SUMS for ${LATEST_TAG}${NC}"
        exit 1
    fi
    EXPECTED_SHA=$(awk -v f="$ASSET_NAME" '$2 == f || $2 == "*" f {print $1; exit}' SHA256SUMS)
    if [ -z "$EXPECTED_SHA" ]; then
        echo -e "${RED}❌ No checksum for ${ASSET_NAME} in SHA256SUMS${NC}"
        exit 1
    fi
    if [ "$(sha256_of "$BINARY_NAME")" != "$EXPECTED_SHA" ]; then
        echo -e "${RED}❌ Checksum mismatch for ${ASSET_NAME}; refusing to install${NC}"
        exit 1
    fi
    
    echo -e "${GREEN}✅ Download completed and checksum verified${NC}"
}

# Verify binary
verify_binary() {
    echo -e "${BLUE}🔒 Verifying binary...${NC}"
    
    # Make executable
    chmod +x "$BINARY_NAME"
    
    # Skip execution test for desktop binary in headless environment
    if [ "$INSTALL_DESKTOP" = true ]; then
        echo -e "${YELLOW}⚠️  Skipping execution test for desktop binary (requires graphical session)${NC}"
        return 0
    fi

    # Test if binary works
    if ! ./"$BINARY_NAME" --help &> /dev/null; then
        echo -e "${RED}❌ Binary verification failed${NC}"
        echo -e "${YELLOW}The downloaded binary may be corrupted or incompatible${NC}"
        exit 1
    fi
    
    echo -e "${GREEN}✅ Binary verified successfully${NC}"
}

# Install binary
install_binary() {
    echo -e "${BLUE}📦 Installing binary...${NC}"
    
    # Check if binary already exists
    if [ -f "${INSTALL_DIR}/${BINARY_NAME}" ]; then
        echo -e "${YELLOW}⚠️  Binary already exists at ${INSTALL_DIR}/${BINARY_NAME}${NC}"
        if [ "$INTERACTIVE" = true ]; then
            prompt "Overwrite? (y/N): "
            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                echo -e "${YELLOW}Installation cancelled${NC}"
                exit 1
            fi
        else
            echo -e "${YELLOW}⚠️  Overwriting existing binary in non-interactive mode${NC}"
        fi
    fi
    
    # Determine sudo prefix
    SUDO_CMD=""
    if [ "$EUID" -ne 0 ]; then
        if command -v sudo &> /dev/null; then
            SUDO_CMD="sudo"
        else
            echo -e "${RED}❌ sudo not available and not running as root${NC}"
            echo -e "${YELLOW}Please run this script as root or install sudo${NC}"
            exit 1
        fi
    fi

    # Copy binary to install directory
    $SUDO_CMD install -m 755 "$BINARY_NAME" "${INSTALL_DIR}/${BINARY_NAME}"
    
    # Verify installation
    if [ -f "${INSTALL_DIR}/${BINARY_NAME}" ]; then
        echo -e "${GREEN}✅ Binary installed successfully${NC}"
        
        # Create bssh alias for CLI
        if [ "$INSTALL_DESKTOP" = false ]; then
            $SUDO_CMD ln -sf "${INSTALL_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/bssh"
            echo -e "${GREEN}✅ Created 'bssh' command alias in ${INSTALL_DIR}${NC}"
        fi
        
        # Install desktop menu shortcut and icon if installing desktop version
        if [ "$INSTALL_DESKTOP" = true ]; then
            echo -e "${BLUE}🎨 Installing desktop menu shortcut and icon...${NC}"

            # Download icon as the invoking user, then install it with sudo.
            # (desktop/src-tauri is a symlink, which raw.githubusercontent.com does not follow.)
            ICON_URL="https://raw.githubusercontent.com/${REPO}/${LATEST_TAG}/crates/gui/icons/128x128.png"
            ICON_DIR="/usr/share/icons/hicolor/128x128/apps"
            if curl -fsSL -o "${TEMP_DIR}/bayesian-ssh-desktop.png" "$ICON_URL"; then
                $SUDO_CMD install -Dm644 "${TEMP_DIR}/bayesian-ssh-desktop.png" "${ICON_DIR}/bayesian-ssh-desktop.png"
            else
                echo -e "${YELLOW}⚠️  Could not download the application icon${NC}"
            fi

            # Create desktop shortcut
            DESKTOP_FILE="/usr/share/applications/bayesian-ssh-desktop.desktop"
            echo -e "[Desktop Entry]\nName=Bayesian SSH\nComment=A fast and lightweight SSH session manager with Kerberos support\nExec=${INSTALL_DIR}/bayesian-ssh-desktop\nIcon=bayesian-ssh-desktop\nTerminal=false\nType=Application\nCategories=Development;Network;\nStartupNotify=true" | $SUDO_CMD tee "$DESKTOP_FILE" >/dev/null

            # Update icon cache
            $SUDO_CMD gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
            echo -e "${GREEN}✅ Desktop menu shortcut and icon installed successfully!${NC}"
        fi
    else
        echo -e "${RED}❌ Installation failed${NC}"
        exit 1
    fi
}

# Build from source option
build_from_source() {
    echo -e "${BLUE}🔨 Building from source...${NC}"
    
    # Check if git is available
    if ! command -v git &> /dev/null; then
        echo -e "${RED}❌ git is required for building from source${NC}"
        exit 1
    fi

    # Check if cargo is available
    if ! command -v cargo &> /dev/null; then
        echo -e "${RED}❌ Rust and Cargo are required for building from source${NC}"
        echo -e "${YELLOW}Please install Rust from https://rustup.rs/ and try again${NC}"
        exit 1
    fi
    
    # Clone repository
    echo -e "${BLUE}📥 Cloning repository...${NC}"
    git clone "https://github.com/${REPO}.git" "$TEMP_DIR"
    cd "$TEMP_DIR"
    
    if [ "$INSTALL_DESKTOP" = true ]; then
        # The desktop build needs the frontend toolchain; the Makefile drives it
        # and installs the CLI, the bssh alias and the GUI (as bayesian-ssh-gui).
        if ! command -v make &> /dev/null || ! command -v npm &> /dev/null; then
            echo -e "${RED}❌ make and npm are required for building the desktop app from source${NC}"
            exit 1
        fi
        echo -e "${BLUE}🔨 Building and installing desktop version with Makefile...${NC}"
        make install
        BINARY_NAME="bayesian-ssh-gui"
    else
        echo -e "${BLUE}🔨 Building CLI version...${NC}"
        cargo build --release --locked --package bayesian-ssh
        cp target/release/bayesian-ssh "$BINARY_NAME"
        install_binary
    fi
    
    echo -e "${GREEN}✅ Build from source completed successfully!${NC}"
}

# Cleanup
cleanup() {
    echo -e "${BLUE}🧹 Cleaning up...${NC}"
    
    if [ -d "$TEMP_DIR" ]; then
        rm -rf "$TEMP_DIR"
    fi
    
    echo -e "${GREEN}✅ Cleanup completed${NC}"
}

# Show success message
show_success() {
    echo ""
    echo -e "${GREEN}🎉 Bayesian SSH installed successfully!${NC}"
    echo ""
    echo -e "${BLUE}📋 Installation Details:${NC}"
    echo -e "  Binary: ${INSTALL_DIR}/${BINARY_NAME}"
    echo -e "  Version: ${LATEST_TAG}"
    echo -e "  Architecture: ${OS}-${ARCH}"
    echo ""
    echo -e "${BLUE}🚀 Quick Start:${NC}"
    echo -e "  ${BINARY_NAME} --help"
    echo -e "  ${BINARY_NAME} add \"My Server\" server.company.com"
    echo -e "  ${BINARY_NAME} connect \"My Server\""
    echo ""
    echo -e "${BLUE}📚 Documentation:${NC}"
    echo -e "  https://github.com/${REPO}#readme"
    echo ""
}

# Show build from source success message
show_build_success() {
    echo ""
    echo -e "${GREEN}🎉 Bayesian SSH built and installed successfully!${NC}"
    echo ""
    echo -e "${BLUE}📋 Installation Details:${NC}"
    echo -e "  Binary: ${INSTALL_DIR}/${BINARY_NAME}"
    echo -e "  Built from source"
    echo -e "  Architecture: ${OS}-${ARCH}"
    echo ""
    echo -e "${BLUE}🚀 Quick Start:${NC}"
    echo -e "  ${BINARY_NAME} --help"
    echo -e "  ${BINARY_NAME} add \"My Server\" server.company.com"
    echo -e "  ${BINARY_NAME} connect \"My Server\""
    echo ""
    echo -e "${BLUE}📚 Documentation:${NC}"
    echo -e "  https://github.com/${REPO}#readme"
    echo ""
}

# Main installation flow
main() {
    echo -e "${BLUE}🚀 Bayesian SSH Installer${NC}"
    echo -e "${BLUE}========================${NC}"
    echo ""
    
    # Check if interactive first
    check_interactive
    
    detect_system
    check_permissions
    check_dependencies
    
    # Choose installation method
    if [ "$INTERACTIVE" = true ]; then
        # Ask user preference
        echo -e "${BLUE}📋 Installation Options:${NC}"
        echo -e "  1. Download pre-built CLI binary (recommended)"
        echo -e "  2. Download pre-built Desktop app (GUI)"
        echo -e "  3. Build CLI binary from source"
        echo -e "  4. Build Desktop app from source"
        echo ""
        prompt "Choose option (1-4): "
        
        if [[ $REPLY =~ ^[2]$ ]]; then
            # Download pre-built Desktop binary
            INSTALL_DESKTOP=true
            BINARY_NAME="bayesian-ssh-desktop"
            get_latest_release
            download_binary
            verify_binary
            install_binary
            cleanup
            show_success
        elif [[ $REPLY =~ ^[3]$ ]]; then
            # Build CLI from source
            build_from_source
            cleanup
            show_build_success
        elif [[ $REPLY =~ ^[4]$ ]]; then
            # Build Desktop from source
            INSTALL_DESKTOP=true
            BINARY_NAME="bayesian-ssh-desktop"
            build_from_source
            cleanup
            show_build_success
        else
            # Default/Option 1: Download pre-built CLI binary
            get_latest_release
            download_binary
            verify_binary
            install_binary
            cleanup
            show_success
        fi
    else
        # Non-interactive mode - use default option
        if [ "$INSTALL_DESKTOP" = true ]; then
            echo -e "${BLUE}📋 Installing pre-built desktop application...${NC}"
        else
            echo -e "${BLUE}📋 Installing pre-built CLI binary...${NC}"
        fi
        get_latest_release
        download_binary
        verify_binary
        install_binary
        cleanup
        show_success
    fi
}

# Run main function
main "$@"

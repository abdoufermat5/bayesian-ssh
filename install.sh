#!/bin/bash

# Bayesian SSH Installer
# Downloads the latest release and installs the CLI (bayesian-ssh + bssh) and
# the desktop app (bayesian-ssh-desktop, with menu entry and icon).
# Usage: curl -fsSL https://raw.githubusercontent.com/abdoufermat5/bayesian-ssh/main/install.sh | bash
# CLI only (servers): ... | bash -s -- --no-gui
# Choose interactively: ... | bash -s -- --interactive

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
REPO="abdoufermat5/bayesian-ssh"
CLI_NAME="bayesian-ssh"
GUI_NAME="bayesian-ssh-desktop"
INSTALL_DIR="/usr/local/bin"
# Private, unpredictable work dir: a fixed /tmp path lets other local users
# pre-create it and swap the binary before it is installed with sudo.
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/bayesian-ssh-install.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT
INTERACTIVE=false
# The desktop app is installed alongside the CLI unless --no-gui is given.
INSTALL_GUI=true
SUDO_CMD=""

usage() {
    echo "Usage: $0 [--interactive] [--no-gui]"
    echo "  (default)      install the CLI and the desktop app"
    echo "  --no-gui       install the CLI only (servers, headless machines)"
    echo "  --interactive  choose between pre-built and source installs"
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --interactive)
            INTERACTIVE=true
            shift
            ;;
        --no-gui)
            INSTALL_GUI=false
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            usage
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

# Use sudo for writes under /usr unless already root.
resolve_sudo() {
    if [ "$EUID" -ne 0 ]; then
        if command -v sudo &> /dev/null; then
            SUDO_CMD="sudo"
        else
            echo -e "${RED}❌ sudo not available and not running as root${NC}"
            echo -e "${YELLOW}Please run this script as root or install sudo${NC}"
            exit 1
        fi
    fi
}

# Download SHA256SUMS once and, when cosign is available, verify it is signed
# by the release workflow. Every asset is then checked against it.
fetch_checksums() {
    cd "$TEMP_DIR"
    RELEASE_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}"

    echo -e "${BLUE}🔒 Fetching release checksums...${NC}"
    if ! curl -fsSL -o SHA256SUMS "${RELEASE_URL}/SHA256SUMS"; then
        echo -e "${RED}❌ Could not download SHA256SUMS for ${LATEST_TAG}${NC}"
        exit 1
    fi

    # Without cosign the checksums are still enforced, but their provenance
    # cannot be verified.
    if command -v cosign &> /dev/null; then
        echo -e "${BLUE}🔏 Verifying SHA256SUMS signature with cosign...${NC}"
        if ! curl -fsSL -o SHA256SUMS.sigstore.json "${RELEASE_URL}/SHA256SUMS.sigstore.json"; then
            echo -e "${RED}❌ Could not download SHA256SUMS.sigstore.json for ${LATEST_TAG}${NC}"
            exit 1
        fi
        if ! cosign verify-blob \
            --bundle SHA256SUMS.sigstore.json \
            --certificate-identity-regexp '^https://github\.com/abdoufermat5/bayesian-ssh/\.github/workflows/release\.yml@refs/(tags/v.+|heads/main)$' \
            --certificate-oidc-issuer https://token.actions.githubusercontent.com \
            SHA256SUMS; then
            echo -e "${RED}❌ Signature verification failed for SHA256SUMS${NC}"
            exit 1
        fi
        echo -e "${GREEN}✅ SHA256SUMS signature verified${NC}"
    else
        echo -e "${YELLOW}⚠️  cosign not found; SHA256SUMS signature not verified (install cosign to verify)${NC}"
    fi
}

# download_asset <release asset> <local file>: download and check its SHA-256.
download_asset() {
    local asset="$1" dest="$2" expected
    cd "$TEMP_DIR"
    echo -e "${BLUE}📥 Downloading ${asset}...${NC}"
    if ! curl -fsSL -o "$dest" "${RELEASE_URL}/${asset}"; then
        echo -e "${RED}❌ Download failed: no ${asset} asset in release ${LATEST_TAG}${NC}"
        echo -e "${YELLOW}Please check the release page manually: https://github.com/${REPO}/releases${NC}"
        exit 1
    fi
    expected=$(awk -v f="$asset" '$2 == f || $2 == "*" f {print $1; exit}' SHA256SUMS)
    if [ -z "$expected" ]; then
        echo -e "${RED}❌ No checksum for ${asset} in SHA256SUMS${NC}"
        exit 1
    fi
    if [ "$(sha256_of "$dest")" != "$expected" ]; then
        echo -e "${RED}❌ Checksum mismatch for ${asset}; refusing to install${NC}"
        exit 1
    fi
    chmod +x "$dest"
    echo -e "${GREEN}✅ ${asset} downloaded and checksum verified${NC}"
}

# confirm_overwrite <path>: in interactive mode, ask before replacing a file.
confirm_overwrite() {
    [ -e "$1" ] || return 0
    if [ "$INTERACTIVE" = true ]; then
        prompt "$1 already exists. Overwrite? (y/N): "
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            echo -e "${YELLOW}Installation cancelled${NC}"
            exit 1
        fi
    else
        echo -e "${YELLOW}⚠️  Replacing existing ${1}${NC}"
    fi
}

# Install the CLI binary from $TEMP_DIR/$CLI_NAME, plus the bssh alias.
install_cli() {
    cd "$TEMP_DIR"
    if ! ./"$CLI_NAME" --help &> /dev/null; then
        echo -e "${RED}❌ The downloaded CLI does not run on this system${NC}"
        exit 1
    fi
    confirm_overwrite "${INSTALL_DIR}/${CLI_NAME}"
    $SUDO_CMD install -m 755 "$CLI_NAME" "${INSTALL_DIR}/${CLI_NAME}"
    $SUDO_CMD ln -sf "${INSTALL_DIR}/${CLI_NAME}" "${INSTALL_DIR}/bssh"
    echo -e "${GREEN}✅ Installed ${INSTALL_DIR}/${CLI_NAME} and the 'bssh' alias${NC}"
}

# Warn when the desktop app's runtime libraries are missing; the binary is
# still installed so it works once they are added.
check_gui_runtime() {
    command -v ldconfig &> /dev/null || return 0
    if ! ldconfig -p 2>/dev/null | grep -q 'libwebkit2gtk-4.1\.so\.0'; then
        echo -e "${YELLOW}⚠️  WebKitGTK 4.1 was not found; the desktop app needs it to start:${NC}"
        echo -e "${YELLOW}     Debian/Ubuntu: sudo apt install libwebkit2gtk-4.1-0 libayatana-appindicator3-1${NC}"
        echo -e "${YELLOW}     Fedora:        sudo dnf install webkit2gtk4.1 libappindicator-gtk3${NC}"
        echo -e "${YELLOW}     Headless machine? Re-run with --no-gui to install only the CLI.${NC}"
    fi
}

# Install the desktop binary from $TEMP_DIR/$GUI_NAME with a menu entry and icon.
install_gui() {
    cd "$TEMP_DIR"
    confirm_overwrite "${INSTALL_DIR}/${GUI_NAME}"
    $SUDO_CMD install -m 755 "$GUI_NAME" "${INSTALL_DIR}/${GUI_NAME}"

    # Download the icon as the invoking user, then install it with sudo.
    # (desktop/src-tauri is a symlink, which raw.githubusercontent.com does not follow.)
    local icon_url="https://raw.githubusercontent.com/${REPO}/${LATEST_TAG}/crates/gui/icons/128x128.png"
    if curl -fsSL -o "${TEMP_DIR}/${GUI_NAME}.png" "$icon_url"; then
        $SUDO_CMD install -Dm644 "${TEMP_DIR}/${GUI_NAME}.png" "/usr/share/icons/hicolor/128x128/apps/${GUI_NAME}.png"
    else
        echo -e "${YELLOW}⚠️  Could not download the application icon${NC}"
    fi

    $SUDO_CMD install -d /usr/share/applications
    printf '%s\n' \
        "[Desktop Entry]" \
        "Name=Bayesian SSH" \
        "Comment=Fast and lightweight SSH session manager" \
        "Exec=${INSTALL_DIR}/${GUI_NAME}" \
        "Icon=${GUI_NAME}" \
        "Terminal=false" \
        "Type=Application" \
        "Categories=Development;Network;" \
        "StartupWMClass=bayesian-ssh-gui" \
        "StartupNotify=true" \
        | $SUDO_CMD tee "/usr/share/applications/${GUI_NAME}.desktop" >/dev/null
    $SUDO_CMD gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true

    echo -e "${GREEN}✅ Installed ${INSTALL_DIR}/${GUI_NAME} with its menu entry and icon${NC}"
    check_gui_runtime
}

# Pre-built install of the CLI and, unless --no-gui, the desktop app.
install_prebuilt() {
    if [ "$OS" != "linux" ]; then
        echo -e "${RED}❌ Pre-built releases are only published for Linux; use a source build${NC}"
        exit 1
    fi
    get_latest_release
    resolve_sudo
    fetch_checksums
    download_asset "${CLI_NAME}-${OS}-${ARCH}" "$CLI_NAME"
    if [ "$INSTALL_GUI" = true ]; then
        download_asset "${GUI_NAME}-${OS}-${ARCH}" "$GUI_NAME"
    fi
    # Install only after every download verified, so a failure leaves nothing half-installed.
    install_cli
    if [ "$INSTALL_GUI" = true ]; then
        install_gui
    fi
}

# Build from source: the CLI, plus the desktop app unless --no-gui.
build_from_source() {
    echo -e "${BLUE}🔨 Building from source...${NC}"

    for tool in git cargo; do
        if ! command -v "$tool" &> /dev/null; then
            echo -e "${RED}❌ ${tool} is required for building from source${NC}"
            [ "$tool" = cargo ] && echo -e "${YELLOW}Please install Rust from https://rustup.rs/ and try again${NC}"
            exit 1
        fi
    done
    if [ "$INSTALL_GUI" = true ] && { ! command -v make &> /dev/null || ! command -v npm &> /dev/null; }; then
        echo -e "${RED}❌ make and npm are required to build the desktop app (or re-run with --no-gui)${NC}"
        exit 1
    fi

    resolve_sudo
    echo -e "${BLUE}📥 Cloning repository...${NC}"
    git clone "https://github.com/${REPO}.git" "$TEMP_DIR/src"
    cd "$TEMP_DIR/src"

    if [ "$INSTALL_GUI" = true ]; then
        # The Makefile builds the frontend and installs the CLI, the bssh alias
        # and the desktop app (as bayesian-ssh-gui).
        make install INSTALL_DIR="$INSTALL_DIR"
    else
        cargo build --release --locked --package bayesian-ssh
        cp target/release/bayesian-ssh "$TEMP_DIR/$CLI_NAME"
        install_cli
    fi
    LATEST_TAG="built from source"
    DESKTOP_HINT="run 'bssh desktop' (installed as ${INSTALL_DIR}/bayesian-ssh-gui)"
}

show_success() {
    echo ""
    echo -e "${GREEN}🎉 Bayesian SSH installed successfully!${NC}"
    echo ""
    echo -e "${BLUE}📋 Installation Details:${NC}"
    echo -e "  CLI:     ${INSTALL_DIR}/${CLI_NAME} (alias: bssh)"
    if [ "$INSTALL_GUI" = true ]; then
        echo -e "  Desktop: ${DESKTOP_HINT:-in your application menu, or run 'bssh desktop'}"
    fi
    echo -e "  Version: ${LATEST_TAG}"
    echo -e "  Architecture: ${OS}-${ARCH}"
    echo ""
    echo -e "${BLUE}🚀 Quick Start:${NC}"
    echo -e "  bssh --help"
    echo -e "  bssh add \"My Server\" server.company.com"
    echo -e "  bssh connect \"My Server\""
    echo ""
    echo -e "${BLUE}📚 Documentation:${NC}"
    echo -e "  https://abdoufermat5.github.io/bayesian-ssh/"
    echo ""
}

# Main installation flow
main() {
    echo -e "${BLUE}🚀 Bayesian SSH Installer${NC}"
    echo -e "${BLUE}========================${NC}"
    echo ""

    check_interactive
    detect_system
    check_permissions
    check_dependencies

    if [ "$INTERACTIVE" = true ]; then
        echo -e "${BLUE}📋 Installation Options:${NC}"
        echo -e "  1. Download CLI + desktop app (recommended)"
        echo -e "  2. Download CLI only"
        echo -e "  3. Build CLI + desktop app from source"
        echo -e "  4. Build CLI only from source"
        echo ""
        prompt "Choose option (1-4): "
        case "$REPLY" in
            2) INSTALL_GUI=false; install_prebuilt ;;
            3) INSTALL_GUI=true; build_from_source ;;
            4) INSTALL_GUI=false; build_from_source ;;
            *) INSTALL_GUI=true; install_prebuilt ;;
        esac
    else
        if [ "$INSTALL_GUI" = true ]; then
            echo -e "${BLUE}📋 Installing the CLI and the desktop app...${NC}"
        else
            echo -e "${BLUE}📋 Installing the CLI only...${NC}"
        fi
        install_prebuilt
    fi

    show_success
}

# Run main function
main "$@"

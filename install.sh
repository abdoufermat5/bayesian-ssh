#!/bin/bash

# Bayesian SSH Installer
# Downloads the latest release and installs the CLI (bayesian-ssh + bssh) and
# the desktop app (bayesian-ssh-desktop, with menu entry and icon).
# Usage: curl -fsSL https://bssh.abdoufermat.dev/install.sh | bash
# CLI only (servers): ... | bash -s -- --no-gui
# Choose interactively: ... | bash -s -- --interactive

set -e

# Configuration
REPO="abdoufermat5/bayesian-ssh"
DOCS_URL="https://bssh.abdoufermat.dev"
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
SIGNATURE="not checked (install cosign to verify it)"

# Colors only on a terminal, and never with NO_COLOR (https://no-color.org).
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    BOLD=$'\033[1m' DIM=$'\033[2m' RED=$'\033[31m' YELLOW=$'\033[33m'
    BLUE=$'\033[34m' GREEN=$'\033[32m' NC=$'\033[0m'
else
    BOLD="" DIM="" RED="" YELLOW="" BLUE="" GREEN="" NC=""
fi

# One line per step; details are indented under it.
step() { printf '%s==>%s %s%s%s\n' "$BLUE" "$NC" "$BOLD" "$*" "$NC"; }
info() { printf '    %s\n' "$*"; }
ok()   { printf '    %s✓%s %s\n' "$GREEN" "$NC" "$*"; }
warn() { printf '    %s!%s %s\n' "$YELLOW" "$NC" "$*" >&2; }
# die <message> [hint...]: stop with an error and what to do about it.
die() {
    printf '%serror:%s %s\n' "$RED" "$NC" "$1" >&2
    shift
    for hint in "$@"; do printf '       %s\n' "$hint" >&2; done
    exit 1
}

usage() {
    cat <<EOF
Install Bayesian SSH from the latest release.

Usage: install.sh [--no-gui] [--interactive]

  (default)      the CLI (bayesian-ssh, bssh) and the desktop app
  --no-gui       the CLI only, for servers and headless machines
  --interactive  choose between pre-built and source installs

Installs to ${INSTALL_DIR} (sudo is used if needed). Set NO_COLOR=1 for plain output.
Docs: ${DOCS_URL}/start/installation/
EOF
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
            usage >&2
            echo >&2
            die "unknown option: $1"
            ;;
    esac
done

# Read a single-key answer from the terminal. When piped from curl, stdin is
# the script itself, so prompts must read from /dev/tty.
prompt() {
    REPLY=""
    read -p "    $1" -n 1 -r < /dev/tty || true
    echo
}

# Detect system architecture and OS
detect_system() {
    case "$(uname -m)" in
        x86_64|amd64) ARCH="x86_64" ;;
        aarch64|arm64) ARCH="aarch64" ;;
        armv7l|armv8l) ARCH="arm" ;;
        *) die "unsupported architecture: $(uname -m)" \
               "Pre-built releases exist for x86_64 and aarch64; see ${DOCS_URL}/start/installation/ to build from source." ;;
    esac
    case "$(uname -s)" in
        Linux) OS="linux" ;;
        Darwin) OS="macos" ;;
        *) die "unsupported operating system: $(uname -s)" ;;
    esac
}

# Check if running as root
check_permissions() {
    if [ "$EUID" -eq 0 ]; then
        warn "running as root: the files are installed system-wide either way, sudo isn't needed"
        if [ "$INTERACTIVE" = true ]; then
            prompt "Continue as root? [y/N] "
            [[ $REPLY =~ ^[Yy]$ ]] || die "cancelled"
        fi
    fi
}

# Check dependencies
check_dependencies() {
    command -v curl &> /dev/null || die "curl is required" "Install it with your package manager, then run this again."
    # Downloads are verified against SHA256SUMS.
    if ! command -v sha256sum &> /dev/null && ! command -v shasum &> /dev/null; then
        die "sha256sum (or shasum) is required to verify the downloads" "Install coreutils, then run this again."
    fi
}

# Get latest release info
get_latest_release() {
    if command -v jq &> /dev/null; then
        LATEST_TAG=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | jq -r '.tag_name')
    else
        # Fallback: /releases/latest redirects to /releases/tag/<latest tag>
        LATEST_TAG=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest")
        LATEST_TAG="${LATEST_TAG##*/tag/}"
    fi

    # The tag is interpolated into download URLs; accept only plain version tags.
    if [[ ! "$LATEST_TAG" =~ ^v[0-9][0-9A-Za-z.+-]*$ ]]; then
        die "could not find the latest release" \
            "Check your connection, or download it from https://github.com/${REPO}/releases"
    fi
    VERSION="${LATEST_TAG#v}"
}

# The version already installed, if any ("" when none).
installed_version() {
    "${INSTALL_DIR}/${CLI_NAME}" --version 2>/dev/null | awk '{print $2}' || true
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
        command -v sudo &> /dev/null || die "writing to ${INSTALL_DIR} needs root, and sudo isn't installed" \
            "Run this script as root, or install sudo."
        SUDO_CMD="sudo"
    fi
}

# Download SHA256SUMS once and, when cosign is available, verify it is signed
# by the release workflow. Every asset is then checked against it.
fetch_checksums() {
    cd "$TEMP_DIR"
    RELEASE_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}"

    curl -fsSL -o SHA256SUMS "${RELEASE_URL}/SHA256SUMS" \
        || die "could not download SHA256SUMS for ${LATEST_TAG}"

    # Without cosign the checksums are still enforced, but their provenance
    # cannot be verified.
    if command -v cosign &> /dev/null; then
        curl -fsSL -o SHA256SUMS.sigstore.json "${RELEASE_URL}/SHA256SUMS.sigstore.json" \
            || die "could not download SHA256SUMS.sigstore.json for ${LATEST_TAG}"
        if ! cosign verify-blob \
            --bundle SHA256SUMS.sigstore.json \
            --certificate-identity-regexp '^https://github\.com/abdoufermat5/bayesian-ssh/\.github/workflows/release\.yml@refs/(tags/v.+|heads/main)$' \
            --certificate-oidc-issuer https://token.actions.githubusercontent.com \
            SHA256SUMS > /dev/null 2>&1; then
            die "the release's SHA256SUMS signature did not verify; nothing was installed" \
                "Report it at https://github.com/${REPO}/issues"
        fi
        SIGNATURE="verified with cosign"
        ok "release signature verified"
    fi
}

# download_asset <release asset> <local file>: download and check its SHA-256.
download_asset() {
    local asset="$1" dest="$2" expected progress=(-s)
    cd "$TEMP_DIR"
    # A progress bar on a terminal: the desktop binary is large.
    [ -t 2 ] && progress=(--progress-bar)
    info "${asset}"
    if ! curl -fSL "${progress[@]}" -o "$dest" "${RELEASE_URL}/${asset}"; then
        die "${LATEST_TAG} has no ${asset} download" \
            "See https://github.com/${REPO}/releases for what is available."
    fi
    expected=$(awk -v f="$asset" '$2 == f || $2 == "*" f {print $1; exit}' SHA256SUMS)
    [ -n "$expected" ] || die "${asset} is not listed in SHA256SUMS; refusing to install it"
    [ "$(sha256_of "$dest")" = "$expected" ] \
        || die "${asset} does not match its checksum; refusing to install it" \
               "Run this again; if it persists, report it at https://github.com/${REPO}/issues"
    chmod +x "$dest"
}

# confirm_overwrite <path>: in interactive mode, ask before replacing a file.
confirm_overwrite() {
    [ -e "$1" ] || return 0
    if [ "$INTERACTIVE" = true ]; then
        prompt "Replace $1? [y/N] "
        [[ $REPLY =~ ^[Yy]$ ]] || die "cancelled"
    fi
}

# Install the CLI binary from $TEMP_DIR/$CLI_NAME, plus the bssh alias.
install_cli() {
    cd "$TEMP_DIR"
    ./"$CLI_NAME" --help &> /dev/null || die "the downloaded CLI does not run on this system (${OS}-${ARCH})"
    confirm_overwrite "${INSTALL_DIR}/${CLI_NAME}"
    $SUDO_CMD install -m 755 "$CLI_NAME" "${INSTALL_DIR}/${CLI_NAME}"
    $SUDO_CMD ln -sf "${INSTALL_DIR}/${CLI_NAME}" "${INSTALL_DIR}/bssh"
    ok "${INSTALL_DIR}/${CLI_NAME}, and bssh"
}

# Warn when the desktop app's runtime libraries are missing; the binary is
# still installed so it works once they are added.
check_gui_runtime() {
    command -v ldconfig &> /dev/null || return 0
    if ! ldconfig -p 2>/dev/null | grep -q 'libwebkit2gtk-4.1\.so\.0'; then
        warn "the desktop app needs WebKitGTK 4.1, which isn't installed:"
        info "  Debian/Ubuntu: sudo apt install libwebkit2gtk-4.1-0 libayatana-appindicator3-1"
        info "  Fedora:        sudo dnf install webkit2gtk4.1 libappindicator-gtk3"
        info "  On a server? Run the installer again with --no-gui."
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
        warn "could not download the application icon; the menu entry has none"
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

    ok "${INSTALL_DIR}/${GUI_NAME}, with a menu entry"
    check_gui_runtime
}

# What is about to happen, before sudo asks for a password.
show_plan() {
    local what="the CLI and the desktop app" current
    [ "$INSTALL_GUI" = true ] || what="the CLI"
    current=$(installed_version)
    if [ -z "$current" ]; then
        info "Installing ${what}, version ${VERSION}, for ${OS}-${ARCH}"
    elif [ "$current" = "$VERSION" ]; then
        info "Reinstalling ${what}, version ${VERSION} (already installed)"
    else
        info "Updating ${what} from ${current} to ${VERSION}"
    fi
    info "into ${INSTALL_DIR}${SUDO_CMD:+ (sudo will ask for your password)}"
}

# Pre-built install of the CLI and, unless --no-gui, the desktop app.
install_prebuilt() {
    [ "$OS" = "linux" ] || die "pre-built releases are only published for Linux" \
        "Build from source instead: ${DOCS_URL}/start/installation/"
    get_latest_release
    resolve_sudo
    show_plan
    echo

    step "Downloading"
    fetch_checksums
    download_asset "${CLI_NAME}-${OS}-${ARCH}" "$CLI_NAME"
    if [ "$INSTALL_GUI" = true ]; then
        download_asset "${GUI_NAME}-${OS}-${ARCH}" "$GUI_NAME"
    fi
    ok "checksums match"

    # Install only after every download verified, so a failure leaves nothing half-installed.
    step "Installing"
    install_cli
    if [ "$INSTALL_GUI" = true ]; then
        install_gui
    fi
}

# Build from source: the CLI, plus the desktop app unless --no-gui.
build_from_source() {
    for tool in git cargo; do
        if ! command -v "$tool" &> /dev/null; then
            [ "$tool" = cargo ] && die "cargo is required to build from source" "Install Rust from https://rustup.rs/, then run this again."
            die "git is required to build from source"
        fi
    done
    if [ "$INSTALL_GUI" = true ] && { ! command -v make &> /dev/null || ! command -v npm &> /dev/null; }; then
        die "make and npm are required to build the desktop app" "Or build the CLI only with --no-gui."
    fi

    resolve_sudo
    step "Building from source (this takes a few minutes)"
    git clone --quiet "https://github.com/${REPO}.git" "$TEMP_DIR/src"
    cd "$TEMP_DIR/src"

    if [ "$INSTALL_GUI" = true ]; then
        # The Makefile builds the frontend and installs the CLI, the bssh alias
        # and the desktop app (as bayesian-ssh-gui).
        make install INSTALL_DIR="$INSTALL_DIR"
    else
        cargo build --release --locked --package bayesian-ssh
        cp target/release/bayesian-ssh "$TEMP_DIR/$CLI_NAME"
        step "Installing"
        install_cli
    fi
    VERSION="$(installed_version)"
    VERSION="${VERSION:-unknown}, built from source"
    SIGNATURE="not applicable (built from source)"
    DESKTOP_HINT="run 'bssh desktop' (installed as ${INSTALL_DIR}/bayesian-ssh-gui)"
}

show_success() {
    echo
    printf '%sBayesian SSH %s is installed.%s\n' "$GREEN$BOLD" "$VERSION" "$NC"
    echo
    info "CLI       bssh (or ${CLI_NAME})"
    if [ "$INSTALL_GUI" = true ]; then
        info "Desktop   ${DESKTOP_HINT:-in your application menu, or 'bssh desktop'}"
    fi
    info "Signature ${SIGNATURE}"
    echo
    info "Get started:"
    info "  bssh import                     ${DIM}# hosts from ~/.ssh/config${NC}"
    info "  bssh add web web.example.com    ${DIM}# or add one${NC}"
    info "  bssh connect web"
    echo
    info "Docs: ${DOCS_URL}"
}

# Main installation flow
main() {
    printf '%sBayesian SSH installer%s\n\n' "$BOLD" "$NC"

    detect_system
    check_permissions
    check_dependencies

    if [ "$INTERACTIVE" = true ]; then
        info "1) Download the CLI and the desktop app (recommended)"
        info "2) Download the CLI only"
        info "3) Build the CLI and the desktop app from source"
        info "4) Build the CLI only from source"
        echo
        prompt "Choose [1-4, default 1]: "
        echo
        case "$REPLY" in
            2) INSTALL_GUI=false; install_prebuilt ;;
            3) INSTALL_GUI=true; build_from_source ;;
            4) INSTALL_GUI=false; build_from_source ;;
            *) INSTALL_GUI=true; install_prebuilt ;;
        esac
    else
        install_prebuilt
    fi

    show_success
}

# Run main function
main "$@"

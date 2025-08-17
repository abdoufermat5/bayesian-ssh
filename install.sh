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
TEMP_DIR="/tmp/bayesian-ssh-install"
INTERACTIVE=false

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --interactive)
            INTERACTIVE=true
            shift
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--interactive]"
            exit 1
            ;;
    esac
done

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
            read -p "Continue anyway? (y/N): " -n 1 -r
            echo
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
    
    # Check for wget (alternative to curl)
    if ! command -v wget &> /dev/null; then
        echo -e "${YELLOW}⚠️  wget not found (will use curl)${NC}"
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
        if [ "$LATEST_TAG" = "null" ] || [ -z "$LATEST_TAG" ]; then
            echo -e "${RED}❌ Failed to get latest release${NC}"
            exit 1
        fi
    else
        # Fallback: scrape the releases page
        LATEST_TAG=$(curl -fsSL "https://github.com/${REPO}/releases" | grep -o 'tag/[^"]*' | head -1 | sed 's|tag/||')
        if [ -z "$LATEST_TAG" ]; then
            echo -e "${RED}❌ Failed to get latest release${NC}"
            exit 1
        fi
    fi
    
    echo -e "${GREEN}✅ Latest release: ${LATEST_TAG}${NC}"
}

# Download binary
download_binary() {
    echo -e "${BLUE}📥 Downloading binary...${NC}"
    
    # Create temp directory
    mkdir -p "$TEMP_DIR"
    cd "$TEMP_DIR"
    
    # Construct download URL
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/bayesian-ssh-${OS}-${ARCH}"
    
    echo -e "${BLUE}📡 Downloading from: ${DOWNLOAD_URL}${NC}"
    
    # Download binary
    if command -v curl &> /dev/null; then
        curl -fsSL -o "$BINARY_NAME" "$DOWNLOAD_URL"
    else
        wget -q -O "$BINARY_NAME" "$DOWNLOAD_URL"
    fi
    
    if [ ! -f "$BINARY_NAME" ]; then
        echo -e "${RED}❌ Download failed${NC}"
        echo -e "${YELLOW}Trying alternative download method...${NC}"
        
        # Try alternative URL format
        ALT_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/bayesian-ssh"
        if command -v curl &> /dev/null; then
            curl -fsSL -o "$BINARY_NAME" "$ALT_URL"
        else
            wget -q -O "$BINARY_NAME" "$ALT_URL"
        fi
        
        if [ ! -f "$BINARY_NAME" ]; then
            echo -e "${RED}❌ All download methods failed${NC}"
            echo -e "${YELLOW}Please check the release page manually: https://github.com/${REPO}/releases${NC}"
            exit 1
        fi
    fi
    
    echo -e "${GREEN}✅ Download completed${NC}"
}

# Verify binary
verify_binary() {
    echo -e "${BLUE}🔒 Verifying binary...${NC}"
    
    # Make executable
    chmod +x "$BINARY_NAME"
    
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
            read -p "Overwrite? (y/N): " -n 1 -r
            echo
            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                echo -e "${YELLOW}Installation cancelled${NC}"
                exit 1
            fi
        else
            echo -e "${YELLOW}⚠️  Overwriting existing binary in non-interactive mode${NC}"
        fi
    fi
    
    # Copy binary to install directory
    if [ "$EUID" -eq 0 ]; then
        # Running as root
        cp "$BINARY_NAME" "${INSTALL_DIR}/${BINARY_NAME}"
    else
        # Not running as root, use sudo
        if command -v sudo &> /dev/null; then
            sudo cp "$BINARY_NAME" "${INSTALL_DIR}/${BINARY_NAME}"
        else
            echo -e "${RED}❌ sudo not available and not running as root${NC}"
            echo -e "${YELLOW}Please run this script as root or install sudo${NC}"
            exit 1
        fi
    fi
    
    # Verify installation
    if [ -f "${INSTALL_DIR}/${BINARY_NAME}" ]; then
        echo -e "${GREEN}✅ Binary installed successfully${NC}"
    else
        echo -e "${RED}❌ Installation failed${NC}"
        exit 1
    fi
}

# Build from source option
build_from_source() {
    echo -e "${BLUE}🔨 Building from source...${NC}"
    
    # Check if make is available
    if ! command -v make &> /dev/null; then
        echo -e "${RED}❌ make is required for building from source${NC}"
        echo -e "${YELLOW}Please install make and try again${NC}"
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
    
    # Build using Makefile
    echo -e "${BLUE}🔨 Building with Makefile...${NC}"
    make release
    
    # Install using Makefile
    echo -e "${BLUE}📦 Installing with Makefile...${NC}"
    make install
    
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
        echo -e "  1. Download pre-built binary (recommended)"
        echo -e "  2. Build from source"
        echo ""
        read -p "Choose option (1 or 2): " -n 1 -r
        echo
        
        if [[ $REPLY =~ ^[2]$ ]]; then
            # Build from source
            build_from_source
            cleanup
            show_build_success
        else
            # Download pre-built binary
            get_latest_release
            download_binary
            verify_binary
            install_binary
            cleanup
            show_success
        fi
    else
        # Non-interactive mode - use default option
        echo -e "${BLUE}📋 Using default option: Download pre-built binary${NC}"
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

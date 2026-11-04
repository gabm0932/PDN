#!/bin/bash
# PDN One-Line Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/michaelamoetoevsky/PDN/main/install.sh | bash

set -e

# Writable temp dir: $TMPDIR, else $PREFIX/tmp on Termux, else /tmp on Linux hosts.
: "${TMPDIR:=${PREFIX:+$PREFIX/tmp}}"
: "${TMPDIR:=/tmp}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Configuration
REPO="michaelamoetoevsky/PDN"
INSTALL_DIR="${HOME}/.local/bin"
RELEASE_URL="https://github.com/${REPO}/releases/latest"

# Detect architecture
ARCH=$(uname -m)
case $ARCH in
    aarch64|arm64) TARGET="aarch64-linux-android" ;;
    x86_64|amd64) TARGET="x86_64-linux-android" ;;
    armv7l|armhf) TARGET="armv7-linux-androideabi" ;;
    *) echo -e "${RED}Unsupported architecture: $ARCH${NC}"; exit 1 ;;
esac

echo -e "${BLUE}╔══════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║     PDN - PRoot Distro Next Installer    ║${NC}"
echo -e "${BLUE}╚══════════════════════════════════════════╝${NC}"
echo ""

# Check if running in Termux
if [[ ! -d "/data/data/com.termux" ]]; then
    echo -e "${YELLOW}Warning: This installer is designed for Termux on Android.${NC}"
    echo -e "${YELLOW}Continuing anyway...${NC}"
    echo ""
fi

# Check dependencies
echo -e "${BLUE}Checking dependencies...${NC}"
for cmd in curl tar dpkg; do
    if ! command -v $cmd &> /dev/null; then
        echo -e "${RED}Missing dependency: $cmd${NC}"
        echo -e "${YELLOW}Install with: pkg install $cmd${NC}"
        exit 1
    fi
done

# Check for proot and proot-distro
if ! command -v proot &> /dev/null; then
    echo -e "${YELLOW}Installing proot...${NC}"
    pkg install -y proot
fi

if ! command -v proot-distro &> /dev/null; then
    echo -e "${YELLOW}Installing proot-distro...${NC}"
    pkg install -y proot-distro
fi

# Get latest release version
echo -e "${BLUE}Fetching latest release...${NC}"
LATEST_VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)

if [[ -z "$LATEST_VERSION" ]]; then
    echo -e "${RED}Failed to fetch latest version${NC}"
    exit 1
fi

echo -e "${GREEN}Latest version: $LATEST_VERSION${NC}"

# Download URL
DEB_URL="https://github.com/${REPO}/releases/download/${LATEST_VERSION}/pdn_${LATEST_VERSION#v}_${TARGET}.deb"
BIN_URL="https://github.com/${REPO}/releases/download/${LATEST_VERSION}/podnext-${TARGET}"

# Try .deb first (preferred for Termux)
echo -e "${BLUE}Downloading PDN package...${NC}"
TEMP_DEB="$TMPDIR/pdn_${LATEST_VERSION#v}_${TARGET}.deb"

if curl -fsSL -o "$TEMP_DEB" "$DEB_URL" 2>/dev/null; then
    echo -e "${GREEN}Downloaded .deb package${NC}"
    echo -e "${BLUE}Installing via dpkg...${NC}"
    dpkg -i "$TEMP_DEB" 2>/dev/null || {
        echo -e "${YELLOW}Fixing dependencies...${NC}"
        apt install -f -y
    }
    rm -f "$TEMP_DEB"
    echo -e "${GREEN}Installation complete!${NC}"
else
    echo -e "${YELLOW}.deb not available, trying binary...${NC}"
    
    # Fallback to binary
    mkdir -p "$INSTALL_DIR"
    TEMP_BIN="$TMPDIR/podnext-${TARGET}"
    
    if curl -fsSL -o "$TEMP_BIN" "$BIN_URL" 2>/dev/null; then
        chmod +x "$TEMP_BIN"
        mv "$TEMP_BIN" "$INSTALL_DIR/podnext"
        
        # Also get daemon and gateway if available
        for BIN in podnext-daemon pn-gateway; do
            BIN_URL="https://github.com/${REPO}/releases/download/${LATEST_VERSION}/${BIN}-${TARGET}"
            if curl -fsSL -o "$TMPDIR/${BIN}-${TARGET}" "$BIN_URL" 2>/dev/null; then
                chmod +x "$TMPDIR/${BIN}-${TARGET}"
                mv "$TMPDIR/${BIN}-${TARGET}" "$INSTALL_DIR/${BIN}"
            fi
        done
        
        # Add to PATH if needed
        if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
            echo -e "${YELLOW}Adding $INSTALL_DIR to PATH...${NC}"
            echo "export PATH=\"\$PATH:$INSTALL_DIR\"" >> "$HOME/.bashrc"
            echo "export PATH=\"\$PATH:$INSTALL_DIR\"" >> "$HOME/.zshrc" 2>/dev/null || true
            export PATH="$PATH:$INSTALL_DIR"
        fi
        
        echo -e "${GREEN}Binary installation complete!${NC}"
    else
        echo -e "${RED}Failed to download PDN${NC}"
        exit 1
    fi
fi

# Generate shell completions
if command -v podnext &> /dev/null; then
    echo -e "${BLUE}Generating shell completions...${NC}"
    podnext completion bash > "$HOME/.bash_completion.d/podnext" 2>/dev/null || true
    podnext completion zsh > "$HOME/.zsh/completions/_podnext" 2>/dev/null || true
    podnext completion fish > "$HOME/.config/fish/completions/podnext.fish" 2>/dev/null || true
fi

# Create default service directory
mkdir -p "$HOME/.config/pn-next/services"
mkdir -p "$HOME/.local/run/pn-next"
mkdir -p "$HOME/.local/log/pn-next"

# Create default supervisor config
cat > "$HOME/.config/pn-next/supervisor.toml" << 'EOF'
# PDN Service Supervisor Configuration
services_dir = "/data/data/com.termux/files/home/.config/pn-next/services"
runtime_dir = "/data/data/com.termux/files/home/.local/run/pn-next"
log_dir = "/data/data/com.termux/files/home/.local/log/pn-next"
supervision_interval_ms = 1000
shutdown_timeout_secs = 30
enable_cgroups = true
max_concurrent_ops = 10
EOF

echo ""
echo -e "${GREEN}╔══════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║     PDN Installation Successful!         ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════════╝${NC}"
echo ""
echo -e "${BLUE}Quick Start:${NC}"
echo -e "  ${YELLOW}podnext --help${NC}              # Show all commands"
echo -e "  ${YELLOW}podnext search ubuntu${NC}       # Search for images"
echo -e "  ${YELLOW}podnext install ubuntu${NC}      # Install Ubuntu container"
echo -e "  ${YELLOW}podnext run ubuntu${NC}          # Run Ubuntu container"
echo -e "  ${YELLOW}podnext service --help${NC}      # Service supervisor commands"
echo ""
echo -e "${BLUE}Service Supervisor:${NC}"
echo -e "  ${YELLOW}podnext daemon start${NC}        # Start supervisor daemon"
echo -e "  ${YELLOW}podnext service create web${NC}  # Create service from template"
echo -e "  ${YELLOW}podnext service start web${NC}   # Start service"
echo ""
echo -e "${BLUE}Documentation:${NC}"
echo -e "  https://github.com/${REPO}"
echo ""
echo -e "${YELLOW}Note: Restart your shell or run 'source ~/.bashrc' to use completions${NC}"
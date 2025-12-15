#!/bin/bash
# Install Javy from GitHub releases

set -e

echo "Installing Javy..."

# Detect OS and architecture
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$ARCH" in
    x86_64) ARCH="x86_64" ;;
    arm64|aarch64) ARCH="aarch64" ;;
    *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

# Download URL - use latest v8.0.0
JAVY_VERSION="v8.0.0"

# Map OS names
case "$OS" in
    darwin) OS_NAME="macos" ;;
    linux) OS_NAME="linux" ;;
    *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

# Map architecture
case "$ARCH" in
    x86_64) ARCH_NAME="x86_64" ;;
    aarch64|arm64) ARCH_NAME="arm" ;;
    *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

JAVY_URL="https://github.com/bytecodealliance/javy/releases/download/${JAVY_VERSION}/javy-${ARCH_NAME}-${OS_NAME}-${JAVY_VERSION}.gz"

echo "Downloading Javy from: $JAVY_URL"

# Download to local bin directory
INSTALL_DIR="$HOME/.local/bin"
mkdir -p "$INSTALL_DIR"

curl -L "$JAVY_URL" | gunzip > "$INSTALL_DIR/javy" || {
    echo "Failed to download javy. Trying cargo install method..."
    cargo install --git https://github.com/bytecodealliance/javy javy-cli || {
        echo "Both methods failed. Please install manually:"
        echo "1. Download from: https://github.com/bytecodealliance/javy/releases"
        echo "2. Or try: cargo install --git https://github.com/bytecodealliance/javy javy-cli"
        exit 1
    }
    exit 0
}

chmod +x "$INSTALL_DIR/javy"

# Add to PATH if not already there
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo ""
    echo "⚠️  Add $INSTALL_DIR to your PATH:"
    echo "   export PATH=\"\$PATH:$INSTALL_DIR\""
    echo ""
    echo "Or add to ~/.bashrc or ~/.zshrc:"
    echo "   echo 'export PATH=\"\$PATH:$HOME/.local/bin\"' >> ~/.bashrc"
fi

echo "✅ Javy installed to: $INSTALL_DIR/javy"
echo ""
echo "Verify installation:"
echo "  $INSTALL_DIR/javy --version"


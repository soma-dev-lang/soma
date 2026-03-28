#!/bin/sh
set -e

REPO="anthropics/soma"
VERSION="0.1.0"
INSTALL_DIR="/usr/local/bin"

echo ""
echo "  ╔═══════════════════════════════╗"
echo "  ║  soma installer v${VERSION}        ║"
echo "  ╚═══════════════════════════════╝"
echo ""

# Detect OS and arch
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$ARCH" in
    x86_64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *) echo "  ✗ unsupported architecture: $ARCH"; exit 1 ;;
esac

case "$OS" in
    darwin) PLATFORM="apple-darwin" ;;
    linux) PLATFORM="unknown-linux-gnu" ;;
    *) echo "  ✗ unsupported OS: $OS"; exit 1 ;;
esac

TARGET="${ARCH}-${PLATFORM}"
BINARY_URL="https://github.com/${REPO}/releases/download/v${VERSION}/soma-${TARGET}"

echo "  platform: ${OS} ${ARCH}"
echo "  target:   ${TARGET}"
echo ""

# Check if binary is available, otherwise build from source
echo "  → checking for pre-built binary..."
if command -v curl > /dev/null 2>&1; then
    HTTP_CODE=$(curl -sL -o /dev/null -w "%{http_code}" "$BINARY_URL" 2>/dev/null || echo "000")
else
    HTTP_CODE="000"
fi

if [ "$HTTP_CODE" = "200" ]; then
    echo "  → downloading soma for ${TARGET}..."
    TMP=$(mktemp)
    curl -fsSL "$BINARY_URL" -o "$TMP"
    chmod +x "$TMP"

    if [ -w "$INSTALL_DIR" ]; then
        mv "$TMP" "$INSTALL_DIR/soma"
    else
        echo "  → installing to ${INSTALL_DIR} (requires sudo)..."
        sudo mv "$TMP" "$INSTALL_DIR/soma"
    fi

    echo ""
    echo "  ✓ soma installed to ${INSTALL_DIR}/soma"
else
    echo "  → no pre-built binary found, building from source..."
    echo ""

    # Check dependencies
    if ! command -v cargo > /dev/null 2>&1; then
        echo "  ✗ cargo not found. Install Rust first:"
        echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi

    if ! command -v git > /dev/null 2>&1; then
        echo "  ✗ git not found. Install git first."
        exit 1
    fi

    TMPDIR=$(mktemp -d)
    echo "  → cloning soma..."
    git clone --quiet --depth 1 https://github.com/${REPO}.git "$TMPDIR/soma"

    echo "  → building (this takes ~30 seconds)..."
    cd "$TMPDIR/soma/compiler"
    cargo build --release --quiet

    if [ -w "$INSTALL_DIR" ]; then
        cp target/release/soma "$INSTALL_DIR/soma"
    else
        echo "  → installing to ${INSTALL_DIR} (requires sudo)..."
        sudo cp target/release/soma "$INSTALL_DIR/soma"
    fi

    # Copy stdlib
    SOMA_LIB="${HOME}/.soma"
    mkdir -p "$SOMA_LIB"
    cp -r ../stdlib "$SOMA_LIB/"

    rm -rf "$TMPDIR"

    echo ""
    echo "  ✓ soma built and installed to ${INSTALL_DIR}/soma"
    echo "  ✓ stdlib installed to ${SOMA_LIB}/stdlib"
fi

echo ""
echo "  verify:"
echo "    soma --version"
echo ""
echo "  quick start:"
echo "    echo 'cell App { on hello(name: String) { return \"Hello {name}!\" } }' > app.cell"
echo "    soma run app.cell hello World"
echo ""
echo "  docs: https://soma-lang.dev"
echo ""

#!/bin/sh
set -e

REPO="soma-dev-lang/soma"
VERSION=$(curl -fsSL "https://raw.githubusercontent.com/${REPO}/main/compiler/Cargo.toml" 2>/dev/null | grep '^version' | head -1 | sed 's/.*"\(.*\)"/\1/')
if [ -z "$VERSION" ]; then
    VERSION="0.31.0"
fi
INSTALL_DIR="$HOME/.soma/bin"

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
    darwin) PLATFORM="apple-darwin"; SHELL_RC="$HOME/.zshrc" ;;
    linux) PLATFORM="unknown-linux-gnu"; SHELL_RC="$HOME/.bashrc" ;;
    *) echo "  ✗ unsupported OS: $OS"; exit 1 ;;
esac

# Override shell rc if bash on mac or zsh on linux
if [ -f "$HOME/.bash_profile" ] && [ "$OS" = "darwin" ]; then
    SHELL_RC="$HOME/.zshrc"
fi
if [ -n "$ZSH_VERSION" ]; then
    SHELL_RC="$HOME/.zshrc"
elif [ -n "$BASH_VERSION" ]; then
    if [ "$OS" = "darwin" ]; then
        SHELL_RC="$HOME/.zshrc"
    else
        SHELL_RC="$HOME/.bashrc"
    fi
fi

TARGET="${ARCH}-${PLATFORM}"
BINARY_URL="https://github.com/${REPO}/releases/download/v${VERSION}/soma-${TARGET}"

echo "  platform: ${OS} ${ARCH}"
echo "  target:   ${TARGET}"
echo "  install:  ${INSTALL_DIR}"
echo ""

# Create install directory
mkdir -p "$INSTALL_DIR"

# Check if binary is available, otherwise build from source
echo "  → checking for pre-built binary..."
if command -v curl > /dev/null 2>&1; then
    HTTP_CODE=$(curl -sL -o /dev/null -w "%{http_code}" "$BINARY_URL" 2>/dev/null || echo "000")
else
    HTTP_CODE="000"
fi

if [ "$HTTP_CODE" = "200" ]; then
    echo "  → downloading soma for ${TARGET}..."
    curl -fsSL "$BINARY_URL" -o "$INSTALL_DIR/soma"
    chmod +x "$INSTALL_DIR/soma"
    echo "  ✓ soma downloaded to ${INSTALL_DIR}/soma"
else
    echo "  → no pre-built binary found, building from source..."
    echo ""

    if ! command -v cargo > /dev/null 2>&1; then
        echo "  → Rust not found, installing..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --quiet
        . "$HOME/.cargo/env"
        echo "  ✓ Rust installed"
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

    cp target/release/soma "$INSTALL_DIR/soma"
    chmod +x "$INSTALL_DIR/soma"

    # Copy stdlib
    mkdir -p "$HOME/.soma"
    cp -r ../stdlib "$HOME/.soma/"

    rm -rf "$TMPDIR"

    echo ""
    echo "  ✓ soma built and installed to ${INSTALL_DIR}/soma"
    echo "  ✓ stdlib installed to $HOME/.soma/stdlib"
fi

# Add to PATH if not already there
PATH_LINE="export PATH=\"\$HOME/.soma/bin:\$PATH\""

if echo "$PATH" | grep -q "$HOME/.soma/bin"; then
    echo "  ✓ $INSTALL_DIR already in PATH"
else
    echo ""
    echo "  → adding $INSTALL_DIR to PATH..."

    if [ -f "$SHELL_RC" ]; then
        if ! grep -q '.soma/bin' "$SHELL_RC" 2>/dev/null; then
            echo "" >> "$SHELL_RC"
            echo "# Soma" >> "$SHELL_RC"
            echo "$PATH_LINE" >> "$SHELL_RC"
            echo "  ✓ added to $SHELL_RC"
        else
            echo "  ✓ already in $SHELL_RC"
        fi
    else
        echo "$PATH_LINE" > "$SHELL_RC"
        echo "  ✓ created $SHELL_RC"
    fi

    # Also add to current session
    export PATH="$HOME/.soma/bin:$PATH"
    echo "  ✓ PATH updated for current session"
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

#!/bin/sh
set -e

REPO="soma-dev-lang/soma"
# The version is the LATEST PUBLISHED RELEASE (not whatever `main` says:
# a version bump on main before its release used to send every install to a
# source build). SOMA_VERSION=2.5.0 pins one explicitly.
VERSION="${SOMA_VERSION:-}"
if [ -z "$VERSION" ]; then
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep '"tag_name"' | head -1 | sed 's/.*"v\{0,1\}\([0-9][^"]*\)".*/\1/')
fi
if [ -z "$VERSION" ]; then
    VERSION=$(curl -fsSL "https://soma-lang.dev/version.json" 2>/dev/null | grep '"soma_version"' | head -1 | sed 's/.*: *"\([^"]*\)".*/\1/')
fi
if [ -z "$VERSION" ]; then
    echo "  ✗ cannot find the latest soma release (GitHub API and soma-lang.dev unreachable) — set SOMA_VERSION=x.y.z"
    exit 1
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
    echo "  → downloading soma ${VERSION} for ${TARGET}..."
    TMP_BIN=$(mktemp)
    curl -fsSL "$BINARY_URL" -o "$TMP_BIN"
    # verify against the release's SHA256SUMS before installing anything
    SUMS=$(curl -fsSL "https://github.com/${REPO}/releases/download/v${VERSION}/SHA256SUMS" 2>/dev/null || true)
    EXPECTED=$(echo "$SUMS" | grep "soma-${TARGET}\$" | awk '{print $1}')
    if command -v shasum > /dev/null 2>&1; then
        ACTUAL=$(shasum -a 256 "$TMP_BIN" | awk '{print $1}')
    else
        ACTUAL=$(sha256sum "$TMP_BIN" | awk '{print $1}')
    fi
    if [ -z "$EXPECTED" ]; then
        echo "  ✗ the release has no checksum for soma-${TARGET} — not installing an unverified binary"
        rm -f "$TMP_BIN"; exit 1
    fi
    if [ "$EXPECTED" != "$ACTUAL" ]; then
        echo "  ✗ checksum mismatch for soma-${TARGET}: expected ${EXPECTED}, got ${ACTUAL}"
        rm -f "$TMP_BIN"; exit 1
    fi
    echo "  ✓ checksum verified (${ACTUAL})"
    mv "$TMP_BIN" "$INSTALL_DIR/soma"
    chmod +x "$INSTALL_DIR/soma"
    echo "  ✓ soma downloaded to ${INSTALL_DIR}/soma"
else
    echo "  → no pre-built binary for ${TARGET} in release v${VERSION}: building v${VERSION} from source"
    echo "    (Rust via rustup in \$HOME if missing, then cargo build — about a minute; nothing outside \$HOME)"
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
    git clone --quiet --depth 1 --branch "v${VERSION}" https://github.com/${REPO}.git "$TMPDIR/soma"

    # GMP (BigInt) is needed to build. The installer never runs sudo: it
    # says what to install and stops.
    if ! pkg-config --exists gmp 2>/dev/null && [ ! -f /usr/include/gmp.h ] && [ ! -f /opt/homebrew/include/gmp.h ] && [ ! -f /usr/local/include/gmp.h ]; then
        if [ "$OS" = "darwin" ] && command -v brew > /dev/null 2>&1; then
            echo "  → installing GMP with Homebrew..."
            brew install gmp --quiet 2>/dev/null
        else
            echo "  ✗ GMP (the BigInt library) is missing. Install it, then run this installer again:"
            echo "      Debian/Ubuntu: sudo apt-get install -y build-essential libgmp-dev m4"
            echo "      Fedora:        sudo dnf install -y gmp-devel"
            echo "      Arch:          sudo pacman -S gmp"
            echo "      macOS:         brew install gmp"
            rm -rf "$TMPDIR"; exit 1
        fi
    fi

    echo "  → building (this takes ~60 seconds)..."
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

# Old installations that would shadow ~/.soma/bin: removed inside $HOME,
# only reported elsewhere (the installer never touches system paths)
for OLD_SOMA in "$HOME/.local/bin/soma" "$HOME/bin/soma" "$HOME/.cargo/bin/soma"; do
    if [ -f "$OLD_SOMA" ]; then
        echo "  → removing old installation: $OLD_SOMA"
        rm -f "$OLD_SOMA" 2>/dev/null || echo "  ⚠ could not remove $OLD_SOMA (remove manually)"
    fi
done
if [ -f /usr/local/bin/soma ]; then
    echo "  ⚠ an older /usr/local/bin/soma exists and may shadow this install — remove it: sudo rm /usr/local/bin/soma"
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
            # Prepend to top of file so ~/.soma/bin has highest priority
            TMP_RC=$(mktemp)
            echo "# Soma" > "$TMP_RC"
            echo "$PATH_LINE" >> "$TMP_RC"
            echo "" >> "$TMP_RC"
            cat "$SHELL_RC" >> "$TMP_RC"
            mv "$TMP_RC" "$SHELL_RC"
            echo "  ✓ added to top of $SHELL_RC"
        else
            echo "  ✓ already in $SHELL_RC"
        fi
    else
        echo "# Soma" > "$SHELL_RC"
        echo "$PATH_LINE" >> "$SHELL_RC"
        echo "  ✓ created $SHELL_RC"
    fi
fi

# Ensure ~/.soma/bin is first in PATH for current session
export PATH="$HOME/.soma/bin:$PATH"

echo ""
echo "  ✓ $(${INSTALL_DIR}/soma --version 2>/dev/null || echo 'soma installed')"
echo ""
echo "  ⚠ run 'source ${SHELL_RC}' or open a new terminal to use soma"

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

#!/usr/bin/env bash
# Install code-primer from GitHub releases.
# Usage: curl -fsSL https://raw.githubusercontent.com/INS-JVidal/code-primer/main/install.sh | bash
set -euo pipefail

REPO="INS-JVidal/code-primer"
INSTALL_DIR="${HOME}/.local/bin"

echo ""
echo '    ______          __        ____       _                    '
echo '   / ____/___  ____/ /__     / __ \_____(_)___ ___  ___  _____'
echo '  / /   / __ \/ __  / _ \   / /_/ / ___/ / __ `__ \/ _ \/ ___/'
echo ' / /___/ /_/ / /_/ /  __/  / ____/ /  / / / / / / /  __/ /    '
echo ' \____/\____/\__,_/\___/  /_/   /_/  /_/_/ /_/ /_/\___/_/     '
echo ""

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)   OS_NAME="linux" ;;
    Darwin)  OS_NAME="macos" ;;
    MINGW*|MSYS*|CYGWIN*)
        OS_NAME="windows"
        ;;
    *)
        echo "Unsupported OS: $OS" >&2
        echo "On Windows, download the .zip from https://github.com/${REPO}/releases/latest" >&2
        exit 1
        ;;
esac

case "$ARCH" in
    x86_64|amd64)  ARCH_NAME="x86_64" ;;
    aarch64|arm64) ARCH_NAME="aarch64" ;;
    *)
        echo "Unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac

if [ "$OS_NAME" = "windows" ]; then
    ASSET_NAME="code-primer-${OS_NAME}-${ARCH_NAME}.zip"
    EXE_NAME="code-primer.exe"
else
    ASSET_NAME="code-primer-${OS_NAME}-${ARCH_NAME}.tar.gz"
    EXE_NAME="code-primer"
fi

echo "Detected: ${OS_NAME}-${ARCH_NAME}"

# Get latest release URL
RELEASE_URL="https://api.github.com/repos/${REPO}/releases/latest"
echo "Fetching latest release..."

DOWNLOAD_URL=$(curl -fsSL "$RELEASE_URL" \
    | grep "browser_download_url.*${ASSET_NAME}" \
    | head -1 \
    | cut -d '"' -f 4)

if [ -z "$DOWNLOAD_URL" ]; then
    echo "No pre-built binary found for ${OS_NAME}-${ARCH_NAME}." >&2
    echo "Install from source instead:" >&2
    echo "  cargo install --git https://github.com/${REPO}" >&2
    exit 1
fi

# Download and install
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading ${ASSET_NAME}..."
curl -fsSL "$DOWNLOAD_URL" -o "$TMPDIR/$ASSET_NAME"

echo "Installing to ${INSTALL_DIR}..."
mkdir -p "$INSTALL_DIR"
if [ "$OS_NAME" = "windows" ]; then
    unzip -o "$TMPDIR/$ASSET_NAME" -d "$INSTALL_DIR"
else
    tar xzf "$TMPDIR/$ASSET_NAME" -C "$INSTALL_DIR"
    chmod +x "$INSTALL_DIR/$EXE_NAME"
fi

# Verify
if "$INSTALL_DIR/$EXE_NAME" --version >/dev/null 2>&1; then
    VERSION=$("$INSTALL_DIR/$EXE_NAME" --version)
    echo "Installed: $VERSION"
else
    echo "Installed to: $INSTALL_DIR/$EXE_NAME"
fi

# Check PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
    echo ""
    echo "Add to your shell profile:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
fi

echo ""
echo "Get started:"
echo "  code-primer init ./your-project"
echo "  code-primer generate ./your-project"

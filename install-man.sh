#!/usr/bin/env bash
# Install code-primer man pages to the local man directory.
# Run after `cargo build` or `cargo install --path .`
set -euo pipefail

# Prefer release build, fall back to debug
MAN_SRC=""
for profile in release debug; do
    candidate=$(find "target/$profile/build" -path "*/man/code-primer.1" -print -quit 2>/dev/null || true)
    if [ -n "$candidate" ]; then
        MAN_SRC="$candidate"
        break
    fi
done

if [ -z "$MAN_SRC" ]; then
    echo "Man pages not found. Run 'cargo build --release' first." >&2
    exit 1
fi
MAN_DIR=$(dirname "$MAN_SRC")

# Install to ~/.local/share/man/man1 (user-local, no sudo needed)
DEST="${HOME}/.local/share/man/man1"
mkdir -p "$DEST"

installed=0
for page in "$MAN_DIR"/*.1; do
    cp "$page" "$DEST/"
    installed=$((installed + 1))
done

echo "Installed ${installed} man pages to ${DEST}"
echo "Verify: man code-primer"
echo ""
echo "If 'man code-primer' doesn't work, add to your shell profile:"
echo "  export MANPATH=\"\${HOME}/.local/share/man:\${MANPATH}\""

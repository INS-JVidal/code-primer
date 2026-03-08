#!/usr/bin/env bash
# Test script for code-primer lifecycle commands.
# Run this from a normal terminal (NOT inside Claude Code) so that
# `claude -p` can work with your Max plan subscription.

set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
OUTPUT_DIR="${PROJECT_DIR}/../code-primer-code-primer"

echo "=== code-primer lifecycle test ==="
echo "Project: ${PROJECT_DIR}"
echo "Output:  ${OUTPUT_DIR}"
echo ""

# 1. Clean any previous run
echo "--- clean ---"
code-primer clean "$PROJECT_DIR" 2>&1
echo ""

# 2. Init
echo "--- init ---"
code-primer init "$PROJECT_DIR" 2>&1
echo ""

# 3. Status (should show all files as new)
echo "--- status (pre-generate) ---"
code-primer status "$PROJECT_DIR" 2>&1
echo ""

# 4. Generate (this calls claude -p for each file)
echo "--- generate ---"
code-primer generate "$PROJECT_DIR" 2>&1
echo ""

# 5. Status (should show no changes needed)
echo "--- status (post-generate) ---"
code-primer status "$PROJECT_DIR" 2>&1
echo ""

# 6. Verify
echo "--- verify ---"
code-primer verify "$PROJECT_DIR" 2>&1
echo ""

# 7. Touch a file and refresh
echo "--- touch src/main.rs + refresh ---"
touch "$PROJECT_DIR/src/main.rs"
sleep 1
code-primer refresh "$PROJECT_DIR" 2>&1
echo ""

# 8. Final status
echo "--- status (post-refresh) ---"
code-primer status "$PROJECT_DIR" 2>&1
echo ""

echo "=== done ==="
echo "Output files:"
ls -la "$OUTPUT_DIR"/ 2>/dev/null || echo "(no output dir)"

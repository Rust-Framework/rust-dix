#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────
# RDI — One-command publish script
# Publishes rdi-macros first, then rdi.
# ──────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "==> 1. Running checks..."

cargo fmt --check 2>/dev/null || { echo "ERROR: cargo fmt --check failed. Run 'cargo fmt' first."; exit 1; }

cargo test 2>&1 | tail -5
echo ""

# Run clippy if available
if cargo clippy -- -D warnings 2>/dev/null; then
    echo "==> clippy: passed"
else
    echo "==> clippy: skipped or found warnings (proceeding anyway)"
fi

echo ""
echo "==> 2. Publishing rdi-macros (proc-macro crate)..."
cd "$SCRIPT_DIR/rdi-macros"
cargo publish "$@"
echo "    ✓ rdi-macros published"
echo "    Waiting 15s for crates.io index refresh..."
sleep 15

echo ""
echo "==> 3. Publishing rdi (main crate)..."
cd "$SCRIPT_DIR"
cargo publish "$@"
echo "    ✓ rdi published"

echo ""
echo "==> Done!"
echo "    https://crates.io/crates/rdi"
echo "    https://crates.io/crates/rdi-macros"

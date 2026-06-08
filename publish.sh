#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────
# RDI — One-command publish script
# Publishes lrdi-macros first, then lrdi.
#
# Usage:
#   ./publish.sh                  # publish both crates
#   ./publish.sh --dry-run        # dry run
#   CARGO_REGISTRY_TOKEN=xxx ./publish.sh  # token via env
# ──────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "============================================"
echo "  RDI Publishing Script"
echo "============================================"
echo ""

# ── Pre-checks ──

# Ensure cargo is installed
command -v cargo >/dev/null 2>&1 || { echo "ERROR: cargo not found"; exit 1; }

# Check if logged in (unless --dry-run)
DRY_RUN=""
for arg in "$@"; do
    if [ "$arg" = "--dry-run" ]; then DRY_RUN="--dry-run"; fi
done

if [ -z "$DRY_RUN" ] && [ -z "${CARGO_REGISTRY_TOKEN:-}" ]; then
    echo "==> Checking crates.io login..."
    if ! (cargo login 2>&1 | grep -q "token"); then
        # Try to detect if already logged in by checking if token file exists
        if [ ! -f "$HOME/.cargo/credentials.toml" ] && [ ! -f "$HOME/.cargo/credentials" ]; then
            echo "------------------------------------------------------------"
            echo "  You are not logged into crates.io."
            echo ""
            echo "  To publish, you need an API token:"
            echo "    1. Go to https://crates.io/me"
            echo "    2. Create a new token"
            echo "    3. Run:  cargo login"
            echo ""
            echo "  Or set the token via environment variable (for CI):"
            echo "    CARGO_REGISTRY_TOKEN=xxx ./publish.sh"
            echo "------------------------------------------------------------"
            exit 1
        fi
    fi
    echo "    ✓ logged in"
fi

echo ""

# ── Step 1: Pre-Publish Checks ──

echo "==> 1. Running pre-publish checks..."

echo "    Checking formatting..."
if cargo fmt --check 2>/dev/null; then
    echo "    ✓ cargo fmt"
else
    echo "    ERROR: cargo fmt --check failed. Run 'cargo fmt' first."
    exit 1
fi

echo "    Running tests..."
cargo test 2>&1 | tail -3
echo ""

# Clippy is advisory only — don't block publish
if cargo clippy 2>/dev/null; then
    echo "    ✓ clippy"
else
    echo "    (clippy found issues — proceeding anyway)"
fi

echo ""
echo "    ✓ All checks passed"
echo ""

# ── Step 2: Publish lrdi-macros ──

MACROS_DIR="crates/lrdi-macros"
MACROS_VER=$(grep '^version ' "$MACROS_DIR/Cargo.toml" | sed 's/version = "\(.*\)"/\1/')
echo "==> 2. Publishing lrdi-macros v${MACROS_VER}..."
(cd "$MACROS_DIR" && cargo publish $DRY_RUN --allow-dirty)
echo "    ✓ lrdi-macros published"

if [ -z "$DRY_RUN" ]; then
    echo "    Waiting 15s for crates.io index to refresh..."
    sleep 15
fi

echo ""

# ── Step 3: Publish lrdi ──

RDI_DIR="crates/lrdi"
RDI_VER=$(grep '^version ' "$RDI_DIR/Cargo.toml" | sed 's/version = "\(.*\)"/\1/')
echo "==> 3. Publishing lrdi v${RDI_VER}..."
(cd "$RDI_DIR" && cargo publish $DRY_RUN --allow-dirty)
echo "    ✓ lrdi published"

echo ""
echo "============================================"
echo "  Done!"
echo ""
echo "  https://crates.io/crates/lrdi"
echo "  https://crates.io/crates/lrdi-macros"
echo "============================================"

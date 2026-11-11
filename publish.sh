#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────
# rust-dix — One-command publish script
# Publishes rust-dix-macros first, then rust-dix.
#
# Usage:
#   ./publish.sh                  # publish both crates
#   ./publish.sh --dry-run        # dry run
#   CARGO_REGISTRY_TOKEN=xxx ./publish.sh  # token via env
# ──────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "============================================"
echo "  rust-dix Publishing Script"
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
cargo test --workspace 2>&1 | tail -3
echo ""

echo "    Running clippy (blocking)..."
if ! cargo clippy --workspace --all-targets -- -D warnings; then
    echo "    ERROR: cargo clippy failed. Fix warnings before publishing."
    exit 1
fi
echo "    ✓ clippy"

echo "    Building docs (rustdoc warnings are errors)..."
if ! RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps; then
    echo "    ERROR: rustdoc emitted warnings. Fix docs before publishing."
    exit 1
fi
echo "    ✓ cargo doc"

echo ""
echo "    ✓ All checks passed"
echo ""

# ── Step 2: Publish rust-dix-macros ──

publish_crate() {
  local dir="$1" crate="$2" ver="$3" attempt log
  echo "==> Publishing ${crate} v${ver}..."
  for attempt in 1 2 3 4 5 6; do
    log="$(mktemp)"
    if (cd "$dir" && cargo publish $DRY_RUN --allow-dirty 2>&1 | tee "$log"); then
      rm -f "$log"
      echo "    ✓ ${crate} published"
      return 0
    fi
    if grep -qiE 'already (uploaded|exists)|crate version .*already' "$log"; then
      rm -f "$log"
      echo "    ✓ ${crate} already on crates.io (rerun-safe)"
      return 0
    fi
    rm -f "$log"
    echo "    attempt ${attempt} failed; retrying in 20s (crates.io index propagation)..."
    sleep 20
  done
  echo "    ERROR: failed to publish ${crate}"
  return 1
}

MACROS_DIR="crates/macros"
MACROS_VER=$(grep '^version ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')
publish_crate "$MACROS_DIR" rust-dix-macros "$MACROS_VER"

if [ -z "$DRY_RUN" ]; then
    echo "    Waiting 15s for crates.io index to refresh..."
    sleep 15
fi

echo ""

# ── Step 3: Publish rust-dix ──

RDI_DIR="crates/core"
RDI_VER=$(grep '^version ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')
publish_crate "$RDI_DIR" rust-dix "$RDI_VER"

echo ""
echo "============================================"
echo "  Done!"
echo ""
echo "  https://crates.io/crates/rust-dix"
echo "  https://crates.io/crates/rust-dix-macros"
echo "============================================"

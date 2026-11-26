#!/usr/bin/env bash
set -euo pipefail

# Topological publish order for CraftUI workspace crates.
# Usage:
#   scripts/publish-crates.sh --package-check   # Verifies packaging metadata for every crate offline/locally
#   scripts/publish-crates.sh --dry-run         # Runs `cargo publish --dry-run` on root leaf crate (craftui-core)
#   scripts/publish-crates.sh --publish         # Publishes all crates in topological order (requires CARGO_REGISTRY_TOKEN)

MODE="${1:---package-check}"

CRATES=(
  craftui-core
  craftui-reactive
  craftui-layout
  craftui-text
  craftui-render
  craftui-a11y
  craftui-widgets
  craftui-dsl
  craftui-runtime
  craftui-macros
  craftui-platform-desktop
  craftui-platform-android
  craftui-platform-ios
  craftui-platform-web
  craftui
  craftui-cli
  craftui-lsp
)

case "$MODE" in
  --package-check)
    for pkg in "${CRATES[@]}"; do
      echo "==> Checking package manifest & file list: $pkg"
      cargo package --list --locked --allow-dirty -p "$pkg" >/dev/null
    done
    echo "==> Running crates.io dry-run on root crate: craftui-core"
    cargo publish --dry-run --locked --allow-dirty -p craftui-core
    echo "All ${#CRATES[@]} crates passed package-list & craftui-core dry-run checks."
    ;;
  --dry-run)
    for pkg in "${CRATES[@]}"; do
      echo "==> Dry-run publish: $pkg"
      cargo publish --dry-run --locked -p "$pkg"
    done
    ;;
  --publish)
    for pkg in "${CRATES[@]}"; do
      echo "==> Publishing $pkg to crates.io"
      cargo publish --locked -p "$pkg"
      echo "Waiting 20s for crates.io index propagation..."
      sleep 20
    done
    ;;
  *)
    echo "Usage: $0 [--package-check|--dry-run|--publish]" >&2
    exit 1
    ;;
esac

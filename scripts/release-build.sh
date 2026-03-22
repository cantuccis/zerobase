#!/usr/bin/env bash
# ───────────────────────────────────────────────────────────────────────────
# release-build.sh — Build Zerobase release binaries
#
# Builds a single, self-contained binary with the admin dashboard embedded.
# Supports native builds and cross-compilation via `cross`.
#
# Usage:
#   ./scripts/release-build.sh                        # native release
#   ./scripts/release-build.sh --target <triple>      # cross-compile
#   ./scripts/release-build.sh --all                  # all supported targets
#
# Outputs go to dist/<target>/zerobase[.exe]
# ───────────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
FRONTEND_DIR="$ROOT_DIR/frontend"

# All supported cross-compilation targets
ALL_TARGETS=(
    "x86_64-unknown-linux-gnu"
    "aarch64-unknown-linux-gnu"
    "x86_64-apple-darwin"
    "aarch64-apple-darwin"
    "x86_64-pc-windows-gnu"
)

VERSION="$(cargo metadata --no-deps --format-version=1 2>/dev/null \
    | grep -o '"version":"[^"]*"' | head -1 | cut -d'"' -f4)"
VERSION="${VERSION:-0.0.0}"

# ── Helpers ────────────────────────────────────────────────────────────────

log()  { echo "==> $*"; }
err()  { echo "ERROR: $*" >&2; exit 1; }

usage() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --target <triple>   Build for a specific target triple
  --all               Build for all supported targets
  --skip-frontend     Skip frontend build (use existing dist/)
  --no-strip          Do not strip the binary
  -h, --help          Show this help message

Supported targets:
$(printf '  %s\n' "${ALL_TARGETS[@]}")

Examples:
  $(basename "$0")                                    # native release build
  $(basename "$0") --target x86_64-unknown-linux-gnu  # Linux amd64
  $(basename "$0") --all                              # all targets
  $(basename "$0") --skip-frontend                    # skip npm build
EOF
    exit 0
}

# ── Parse arguments ────────────────────────────────────────────────────────

TARGETS=()
SKIP_FRONTEND=false
STRIP_BINARY=true

while [[ $# -gt 0 ]]; do
    case "$1" in
        --target)
            shift
            [[ $# -gt 0 ]] || err "--target requires a value"
            TARGETS+=("$1")
            ;;
        --all)
            TARGETS=("${ALL_TARGETS[@]}")
            ;;
        --skip-frontend)
            SKIP_FRONTEND=true
            ;;
        --no-strip)
            STRIP_BINARY=false
            ;;
        -h|--help)
            usage
            ;;
        *)
            err "Unknown option: $1"
            ;;
    esac
    shift
done

# Default to native target if none specified
if [[ ${#TARGETS[@]} -eq 0 ]]; then
    NATIVE_TARGET="$(rustc -vV | awk '/^host:/ { print $2 }')"
    TARGETS=("$NATIVE_TARGET")
fi

# ── Build frontend ─────────────────────────────────────────────────────────

if [[ "$SKIP_FRONTEND" == false ]]; then
    log "Building admin dashboard frontend..."
    if [[ ! -f "$FRONTEND_DIR/package.json" ]]; then
        err "Frontend directory not found at $FRONTEND_DIR"
    fi

    cd "$FRONTEND_DIR"

    if command -v pnpm &>/dev/null; then
        pnpm install --frozen-lockfile
        pnpm run build
    elif command -v npm &>/dev/null; then
        npm ci
        npm run build
    else
        err "Neither pnpm nor npm found. Install one to build the frontend."
    fi

    cd "$ROOT_DIR"
    log "Frontend build complete."
else
    log "Skipping frontend build (--skip-frontend)"
    if [[ ! -d "$FRONTEND_DIR/dist" ]]; then
        err "Frontend dist/ not found. Run without --skip-frontend first."
    fi
fi

# ── Verify frontend assets exist ──────────────────────────────────────────

if [[ ! -f "$FRONTEND_DIR/dist/index.html" ]]; then
    err "Frontend build output missing: $FRONTEND_DIR/dist/index.html"
fi

log "Frontend assets verified ($(find "$FRONTEND_DIR/dist" -type f | wc -l | tr -d ' ') files)"

# ── Build each target ─────────────────────────────────────────────────────

build_target() {
    local target="$1"
    local use_cross=false

    # Determine native target
    local native_target
    native_target="$(rustc -vV | awk '/^host:/ { print $2 }')"

    if [[ "$target" != "$native_target" ]]; then
        if command -v cross &>/dev/null; then
            use_cross=true
        else
            log "Warning: 'cross' not installed. Trying cargo with --target."
            log "Install cross with: cargo install cross"
        fi
    fi

    local output_dir="$DIST_DIR/$target"
    mkdir -p "$output_dir"

    log "Building zerobase for $target ($(if $use_cross; then echo 'cross'; else echo 'cargo'; fi))..."

    local build_cmd
    if $use_cross; then
        build_cmd="cross"
    else
        build_cmd="cargo"
    fi

    $build_cmd build \
        --release \
        --target "$target" \
        --package zerobase-server

    # Determine binary name
    local bin_name="zerobase"
    if [[ "$target" == *windows* ]]; then
        bin_name="zerobase.exe"
    fi

    local src_binary="$ROOT_DIR/target/$target/release/$bin_name"
    if [[ ! -f "$src_binary" ]]; then
        err "Binary not found at $src_binary"
    fi

    cp "$src_binary" "$output_dir/$bin_name"

    # Strip binary (unless --no-strip or cross-compiling to different arch)
    if [[ "$STRIP_BINARY" == true && "$target" == "$native_target" ]]; then
        log "Stripping binary..."
        strip "$output_dir/$bin_name" 2>/dev/null || true
    fi

    local size
    size="$(du -h "$output_dir/$bin_name" | cut -f1)"
    log "Built: $output_dir/$bin_name ($size)"

    # Create archive
    local archive_name="zerobase-v${VERSION}-${target}"
    cd "$output_dir"
    if [[ "$target" == *windows* ]]; then
        if command -v zip &>/dev/null; then
            zip "$DIST_DIR/${archive_name}.zip" "$bin_name"
            log "Archive: $DIST_DIR/${archive_name}.zip"
        fi
    else
        tar -czf "$DIST_DIR/${archive_name}.tar.gz" "$bin_name"
        log "Archive: $DIST_DIR/${archive_name}.tar.gz"
    fi
    cd "$ROOT_DIR"
}

# Build all targets
for target in "${TARGETS[@]}"; do
    build_target "$target"
done

# ── Generate checksums ─────────────────────────────────────────────────────

log "Generating checksums..."
cd "$DIST_DIR"
if command -v sha256sum &>/dev/null; then
    sha256sum *.tar.gz *.zip 2>/dev/null > checksums-sha256.txt || true
elif command -v shasum &>/dev/null; then
    shasum -a 256 *.tar.gz *.zip 2>/dev/null > checksums-sha256.txt || true
fi
cd "$ROOT_DIR"

# ── Summary ────────────────────────────────────────────────────────────────

log ""
log "Release build complete!"
log "  Version: v$VERSION"
log "  Targets: ${TARGETS[*]}"
log "  Output:  $DIST_DIR/"
ls -lh "$DIST_DIR/"*.tar.gz "$DIST_DIR/"*.zip 2>/dev/null | while read -r line; do
    log "    $line"
done

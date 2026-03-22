#!/usr/bin/env bash
# docker-test.sh — Verify Docker build and container startup
#
# Tests:
#   1. Docker image builds successfully
#   2. Image size is under 50 MB
#   3. Container starts and serves the API health endpoint
#   4. Container serves the admin dashboard at /_/
#   5. Container stops gracefully
#
# Usage:
#   ./scripts/docker-test.sh            # build & test
#   ./scripts/docker-test.sh --no-build # test existing image

set -euo pipefail

IMAGE_NAME="zerobase-test"
CONTAINER_NAME="zerobase-docker-test"
HOST_PORT=18090
MAX_IMAGE_SIZE_MB=50
STARTUP_TIMEOUT=30

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

pass=0
fail=0

log_pass() { echo -e "  ${GREEN}PASS${NC} $1"; ((pass++)); }
log_fail() { echo -e "  ${RED}FAIL${NC} $1"; ((fail++)); }
log_info() { echo -e "  ${YELLOW}INFO${NC} $1"; }

cleanup() {
    log_info "Cleaning up..."
    docker rm -f "$CONTAINER_NAME" 2>/dev/null || true
}
trap cleanup EXIT

# ── Parse args ───────────────────────────────────────────────────────────────

SKIP_BUILD=false
for arg in "$@"; do
    case "$arg" in
        --no-build) SKIP_BUILD=true ;;
    esac
done

echo "=== Zerobase Docker Tests ==="
echo ""

# ── Test 1: Build ────────────────────────────────────────────────────────────

if [ "$SKIP_BUILD" = false ]; then
    echo "--- Test 1: Docker image builds ---"
    if docker build -t "$IMAGE_NAME" . ; then
        log_pass "Image built successfully"
    else
        log_fail "Image build failed"
        echo ""
        echo "Results: $pass passed, $fail failed"
        exit 1
    fi
else
    echo "--- Test 1: Build skipped (--no-build) ---"
    log_info "Using existing image $IMAGE_NAME"
fi

echo ""

# ── Test 2: Image size ───────────────────────────────────────────────────────

echo "--- Test 2: Image size under ${MAX_IMAGE_SIZE_MB} MB ---"

IMAGE_SIZE_BYTES=$(docker image inspect "$IMAGE_NAME" --format='{{.Size}}')
IMAGE_SIZE_MB=$((IMAGE_SIZE_BYTES / 1024 / 1024))

if [ "$IMAGE_SIZE_MB" -le "$MAX_IMAGE_SIZE_MB" ]; then
    log_pass "Image size: ${IMAGE_SIZE_MB} MB (limit: ${MAX_IMAGE_SIZE_MB} MB)"
else
    log_fail "Image size: ${IMAGE_SIZE_MB} MB (exceeds limit: ${MAX_IMAGE_SIZE_MB} MB)"
fi

echo ""

# ── Test 3: Container starts and serves API ──────────────────────────────────

echo "--- Test 3: Container starts and serves API ---"

docker rm -f "$CONTAINER_NAME" 2>/dev/null || true

docker run -d \
    --name "$CONTAINER_NAME" \
    -p "${HOST_PORT}:8090" \
    -e ZEROBASE__AUTH__TOKEN_SECRET=test-secret-for-docker-validation-only \
    -e ZEROBASE__SERVER__LOG_FORMAT=pretty \
    "$IMAGE_NAME"

# Wait for health
WAITED=0
HEALTHY=false
while [ "$WAITED" -lt "$STARTUP_TIMEOUT" ]; do
    if wget -qO- "http://127.0.0.1:${HOST_PORT}/api/health" 2>/dev/null | grep -q '"ok"'; then
        HEALTHY=true
        break
    fi
    sleep 1
    ((WAITED++))
done

if [ "$HEALTHY" = true ]; then
    log_pass "API health endpoint responding (took ${WAITED}s)"
else
    log_fail "API health endpoint not responding after ${STARTUP_TIMEOUT}s"
    log_info "Container logs:"
    docker logs "$CONTAINER_NAME" 2>&1 | tail -20
fi

echo ""

# ── Test 4: Dashboard served ─────────────────────────────────────────────────

echo "--- Test 4: Admin dashboard served at /_/ ---"

if [ "$HEALTHY" = true ]; then
    HTTP_CODE=$(wget -qO /dev/null -S "http://127.0.0.1:${HOST_PORT}/_/" 2>&1 | grep "HTTP/" | tail -1 | awk '{print $2}')
    if [ "$HTTP_CODE" = "200" ]; then
        log_pass "Dashboard returns HTTP 200"
    else
        log_fail "Dashboard returned HTTP ${HTTP_CODE:-timeout}"
    fi
else
    log_fail "Skipped (API not responding)"
fi

echo ""

# ── Test 5: Graceful shutdown ────────────────────────────────────────────────

echo "--- Test 5: Container stops gracefully ---"

if docker stop --time 10 "$CONTAINER_NAME" >/dev/null 2>&1; then
    EXIT_CODE=$(docker inspect "$CONTAINER_NAME" --format='{{.State.ExitCode}}')
    if [ "$EXIT_CODE" = "0" ]; then
        log_pass "Container exited with code 0"
    else
        log_fail "Container exited with code $EXIT_CODE"
    fi
else
    log_fail "Container did not stop within 10s"
fi

echo ""

# ── Summary ──────────────────────────────────────────────────────────────────

echo "=== Results: $pass passed, $fail failed ==="

if [ "$fail" -gt 0 ]; then
    exit 1
fi

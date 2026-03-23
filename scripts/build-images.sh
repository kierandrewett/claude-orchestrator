#!/usr/bin/env bash
# Build all Claude Code Docker images.
# Usage: scripts/build-images.sh [image-prefix]
set -euo pipefail

PREFIX="${1:-orchestrator/claude-code}"
DOCKER_DIR="$(cd "$(dirname "$0")/.." && pwd)/docker"

echo "Building Docker images with prefix: $PREFIX"
echo ""

build_image() {
    local name="$1"
    local dockerfile="$2"
    local tag="${PREFIX}:${name}"

    echo "==> Building $tag from $dockerfile..."
    docker build \
        --file "$DOCKER_DIR/$dockerfile" \
        --tag "$tag" \
        "$DOCKER_DIR"

    local size
    size=$(docker image inspect "$tag" --format '{{.Size}}' 2>/dev/null | numfmt --to=iec 2>/dev/null || echo "unknown")
    echo "    ✅ $tag (size: $size)"
    echo ""
}

build_image "base"   "Dockerfile.base"
build_image "node"   "Dockerfile.node"
build_image "rust"   "Dockerfile.rust"
build_image "python" "Dockerfile.python"

echo "All images built successfully."
echo ""
echo "Images:"
docker images --filter "reference=${PREFIX}:*" --format "  {{.Repository}}:{{.Tag}}\t{{.Size}}"

#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
IMAGE_NAME="${THAT_AGENT_SANDBOX_IMAGE:-that-agent-sandbox}"
CACHE_DIR="${THAT_SANDBOX_BUILD_CACHE_DIR:-$PROJECT_DIR/.cache/that-sandbox-buildx}"

echo "Building sandbox image: $IMAGE_NAME"
echo "Workspace:              $PROJECT_DIR"

# Create a temporary build context
BUILD_CTX=$(mktemp -d)
trap 'rm -rf "$BUILD_CTX"' EXIT

cp "$SCRIPT_DIR/Dockerfile" "$BUILD_CTX/"

# Copy the workspace source — exclude heavy / irrelevant dirs
rsync -a \
    --exclude='/target' \
    --exclude='/.git' \
    --exclude='/.cache' \
    --exclude='/node_modules' \
    --exclude='/that-agent' \
    --exclude='/agentic-tools' \
    --exclude='/sandbox' \
    "$PROJECT_DIR/" "$BUILD_CTX/"

# Bake built-in skills into the image
if [ -d "$SCRIPT_DIR/skills" ]; then
    cp -r "$SCRIPT_DIR/skills" "$BUILD_CTX/skills"
fi

if docker buildx version >/dev/null 2>&1; then
    mkdir -p "$CACHE_DIR"
    echo "Build cache:            $CACHE_DIR"
    CACHE_FROM_ARGS=()
    if [ -f "$CACHE_DIR/index.json" ]; then
        CACHE_FROM_ARGS=(--cache-from "type=local,src=$CACHE_DIR")
    else
        echo "Build cache status:     cold cache (first run)"
    fi
    docker buildx build \
        --load \
        "${CACHE_FROM_ARGS[@]}" \
        --cache-to "type=local,dest=$CACHE_DIR,mode=max" \
        -t "$IMAGE_NAME" \
        -f "$BUILD_CTX/Dockerfile" \
        "$BUILD_CTX"
else
    DOCKER_BUILDKIT=1 docker build \
        -t "$IMAGE_NAME" \
        -f "$BUILD_CTX/Dockerfile" \
        "$BUILD_CTX"
fi

echo ""
echo "Sandbox image built: $IMAGE_NAME"
echo "Run 'that --sandbox run <task>' to use it"

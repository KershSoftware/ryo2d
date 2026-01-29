#!/usr/bin/env bash
# Ferrum2D Documentation Build Script (Unix/Linux/macOS)
# For Windows, use build-docs.ps1 instead
# Generates combined docs: mdbook (guides) + rustdoc (API reference)
# Usage: 
#   ./build-docs.sh         → Build and open
#   ./build-docs.sh open    → Open last build (no rebuild)
#   ./build-docs.sh serve   → Build and serve with live reload

set -e

COMMAND="${1:-build}"

# If open, just open the last build
if [ "$COMMAND" = "open" ]; then
    if [ -f "docs/book/index.html" ]; then
        if command -v start &> /dev/null; then
            start docs/book/index.html  # Windows git-bash
        elif command -v open &> /dev/null; then
            open docs/book/index.html  # macOS
        elif command -v xdg-open &> /dev/null; then
            xdg-open docs/book/index.html  # Linux
        else
            echo "docs/book/index.html"
        fi
        exit 0
    else
        echo "No documentation found. Run without 'open' to build."
        exit 1
    fi
fi

if [ "$COMMAND" = "serve" ]; then
    echo "Starting documentation server with live reload..."
    mdbook serve docs --open
    exit 0
fi

if [ "$COMMAND" != "build" ]; then
    echo "Unknown command: $COMMAND"
    echo ""
    echo "Usage:"
    echo "  ./build-docs.sh         → Build and open"
    echo "  ./build-docs.sh open    → Open last build (no rebuild)"
    echo "  ./build-docs.sh serve   → Build and serve with live reload"
    exit 1
fi

echo "Building Ferrum2D Documentation..."

# Step 1: Build rustdoc
echo ""
echo "[1/3] Building API documentation (rustdoc)..."
cargo doc --no-deps --document-private-items

# Step 2: Build mdbook
echo ""
echo "[2/3] Building guide documentation (mdbook)..."
mdbook build docs

# Step 3: Copy rustdoc into mdbook output
echo ""
echo "[3/3] Combining documentation..."
rm -rf docs/book/api
cp -r target/doc docs/book/api

echo ""
echo "Documentation built successfully!"
echo "  Guide: docs/book/index.html"
echo "  API:   docs/book/api/ferrum2d/index.html"
echo ""

# Open in browser
if command -v start &> /dev/null; then
    start docs/book/index.html  # Windows git-bash
elif command -v open &> /dev/null; then
    open docs/book/index.html  # macOS
elif command -v xdg-open &> /dev/null; then
    xdg-open docs/book/index.html  # Linux
else
    echo "Built! Open docs/book/index.html in your browser"
fi

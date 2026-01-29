#!/usr/bin/env bash
# Ryo2D Build Script (Unix/Linux/macOS)
# For Windows, use build.ps1 instead
#
# Usage:
#   ./build.sh         - Build in release mode
#   ./build.sh debug   - Build in debug mode
#   ./build.sh check   - Fast compilation check (no binary)

set -e  # Exit on error

MODE="${1:-release}"

echo -e "\033[0;36mRyo2D Build Script\033[0m"
echo -e "\033[0;36m=====================\033[0m"
echo ""

case "$MODE" in
    release)
        echo -e "\033[0;32mBuilding in RELEASE mode (optimized)...\033[0m"
        cargo build --release
        ;;
    debug)
        echo -e "\033[0;33mBuilding in DEBUG mode (fast compile, slow runtime)...\033[0m"
        cargo build
        ;;
    check)
        echo -e "\033[0;36mRunning fast compilation check...\033[0m"
        cargo check
        ;;
    *)
        echo -e "\033[0;31mUnknown mode: $MODE\033[0m"
        echo -e "\033[0;33mValid modes: release, debug, check\033[0m"
        exit 1
        ;;
esac

echo ""
echo -e "\033[0;32mBuild completed successfully!\033[0m"
if [ "$MODE" = "release" ]; then
    echo -e "\033[0;36mBinary location: target/release/ryo2d-demo\033[0m"
elif [ "$MODE" = "debug" ]; then
    echo -e "\033[0;36mBinary location: target/debug/ryo2d-demo\033[0m"
fi

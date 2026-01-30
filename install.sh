#!/usr/bin/env bash
# Ryo2D CLI Installation Script (Unix)
#
# Installs the ryo2d CLI using `cargo install --path .`
# This installs to ~/.cargo/bin, which should already be in your PATH.
#
# Usage:
#   ./install.sh              # Install to ~/.cargo/bin
#   ./install.sh --uninstall  # Uninstall the CLI

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UNINSTALL=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --uninstall|-u)
            UNINSTALL=true
            shift
            ;;
        --help|-h)
            echo "Ryo2D CLI Installer"
            echo ""
            echo "Usage: ./install.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --uninstall, -u    Uninstall the CLI"
            echo "  --help, -h         Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Output helpers
status() {
    printf "%12s %s\n" "$1" "$2"
}

success() {
    printf "\033[32m%12s\033[0m %s\n" "Done" "$1"
}

warning() {
    printf "\033[33m%12s\033[0m %s\n" "Warning" "$1"
}

error() {
    printf "\033[31m%12s\033[0m %s\n" "Error" "$1"
}

# Uninstall mode
if [ "$UNINSTALL" = true ]; then
    status "Uninstalling" "ryo2d CLI"
    
    cd "$SCRIPT_DIR"
    if cargo uninstall ryo2d; then
        success "ryo2d CLI uninstalled"
    fi
    exit 0
fi

# Install mode
echo ""
echo "Ryo2D CLI Installer"
echo "==================="
echo ""

status "Installing" "ryo2d CLI to ~/.cargo/bin..."

cd "$SCRIPT_DIR"
if ! cargo install --path . --bin ryo2d; then
    error "Installation failed"
    exit 1
fi

echo ""
success "ryo2d CLI installed successfully!"
echo ""
echo "Run 'ryo2d --help' to get started."
echo ""

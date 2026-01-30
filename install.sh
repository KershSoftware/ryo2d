#!/usr/bin/env bash
# Ryo2D CLI Installation Script (Unix)
#
# This script builds the ryo2d CLI and creates a symlink for global access.
#
# Usage:
#   ./install.sh              # Install to default location (~/.ryo2d/bin)
#   ./install.sh --uninstall  # Remove the symlink
#   ./install.sh --prefix DIR # Install to custom directory

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY_NAME="ryo2d"
INSTALL_DIR="$HOME/.ryo2d/bin"
UNINSTALL=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --uninstall|-u)
            UNINSTALL=true
            shift
            ;;
        --prefix|-p)
            INSTALL_DIR="$2"
            shift 2
            ;;
        --help|-h)
            echo "Ryo2D CLI Installer"
            echo ""
            echo "Usage: ./install.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --prefix, -p DIR   Install to custom directory (default: ~/.ryo2d/bin)"
            echo "  --uninstall, -u    Remove the installed CLI"
            echo "  --help, -h         Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

TARGET_BINARY="$SCRIPT_DIR/target/release/$BINARY_NAME"
INSTALLED_BINARY="$INSTALL_DIR/$BINARY_NAME"

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
    status "Removing" "$INSTALLED_BINARY"
    
    if [ -L "$INSTALLED_BINARY" ] || [ -f "$INSTALLED_BINARY" ]; then
        rm -f "$INSTALLED_BINARY"
        success "ryo2d CLI uninstalled"
    else
        warning "ryo2d CLI was not installed at $INSTALLED_BINARY"
    fi
    exit 0
fi

# Install mode
echo ""
echo "Ryo2D CLI Installer"
echo "==================="
echo ""

# Step 1: Build release binary
status "Building" "ryo2d CLI (release mode)..."

cd "$SCRIPT_DIR"
if ! cargo build --release --bin ryo2d; then
    error "Build failed"
    exit 1
fi

# Verify binary exists
if [ ! -f "$TARGET_BINARY" ]; then
    error "Binary not found at $TARGET_BINARY"
    echo "Make sure you have a [[bin]] entry for ryo2d in Cargo.toml"
    exit 1
fi

# Step 2: Create install directory
if [ ! -d "$INSTALL_DIR" ]; then
    status "Creating" "$INSTALL_DIR"
    mkdir -p "$INSTALL_DIR"
fi

# Step 3: Create symlink
status "Linking" "$INSTALLED_BINARY -> $TARGET_BINARY"

# Remove existing symlink/file if present
if [ -L "$INSTALLED_BINARY" ] || [ -f "$INSTALLED_BINARY" ]; then
    rm -f "$INSTALLED_BINARY"
fi

ln -s "$TARGET_BINARY" "$INSTALLED_BINARY"

# Step 4: Check if install dir is in PATH
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo ""
    warning "$INSTALL_DIR is not in your PATH"
    echo ""
    
    read -p "Would you like to add it to your shell config now? [Y/n] " response
    if [[ "$response" == "" || "$response" =~ ^[Yy] ]]; then
        # Detect shell and config file
        SHELL_NAME=$(basename "$SHELL")
        case "$SHELL_NAME" in
            bash)
                CONFIG_FILE="$HOME/.bashrc"
                ;;
            zsh)
                CONFIG_FILE="$HOME/.zshrc"
                ;;
            fish)
                CONFIG_FILE="$HOME/.config/fish/config.fish"
                ;;
            *)
                CONFIG_FILE=""
                ;;
        esac
        
        if [ -n "$CONFIG_FILE" ]; then
            echo "" >> "$CONFIG_FILE"
            echo "# Ryo2D CLI" >> "$CONFIG_FILE"
            if [ "$SHELL_NAME" = "fish" ]; then
                echo "fish_add_path $INSTALL_DIR" >> "$CONFIG_FILE"
            else
                echo "export PATH=\"\$PATH:$INSTALL_DIR\"" >> "$CONFIG_FILE"
            fi
            
            status "Updated" "$CONFIG_FILE"
            echo ""
            echo "  Run 'source $CONFIG_FILE' or restart your terminal."
        else
            warning "Unknown shell: $SHELL_NAME"
            echo ""
            echo "Add this to your shell configuration:"
            echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
        fi
    else
        echo ""
        echo "To add manually, add this to your shell configuration:"
        echo ""
        echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
        echo ""
    fi
fi

echo ""
success "ryo2d CLI installed successfully!"
echo ""
echo "Run 'ryo2d --help' to get started."
echo ""

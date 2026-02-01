#!/usr/bin/env bash
# Ryo2D CLI Installation Script (Unix/macOS)
#
# Installs the ryo2d CLI to a dedicated directory and adds it to PATH.
# Installation directory: ~/.ryo2d/bin
#
# Usage:
#   ./install.sh              # Install ryo2d
#   ./install.sh --uninstall  # Uninstall ryo2d

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_DIR="$HOME/.ryo2d/bin"
BINARY_PATH="$INSTALL_DIR/ryo2d"
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
    printf "\033[36m%12s\033[0m %s\n" "$1" "$2"
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

# Check if directory is in PATH
in_path() {
    case ":$PATH:" in
        *":$1:"*) return 0 ;;
        *) return 1 ;;
    esac
}

# Add directory to shell RC files
add_to_path() {
    local dir="$1"
    
    if in_path "$dir"; then
        return 1  # Already in PATH
    fi
    
    echo ""
    echo -e "\033[33mThe installation directory needs to be added to your PATH:\033[0m"
    echo -e "  \033[90m$dir\033[0m"
    echo ""
    read -p "Add to PATH? (Y/n): " response
    
    if [[ "$response" =~ ^[Yy]?$ ]]; then
        local export_line="export PATH=\"\$PATH:$dir\""
        local updated=false
        
        # Add to bash
        if [ -f "$HOME/.bashrc" ]; then
            if ! grep -q "ryo2d/bin" "$HOME/.bashrc"; then
                status "Updating" ".bashrc..."
                echo "" >> "$HOME/.bashrc"
                echo "# Ryo2D CLI" >> "$HOME/.bashrc"
                echo "$export_line" >> "$HOME/.bashrc"
                updated=true
            fi
        fi
        
        # Add to zsh
        if [ -f "$HOME/.zshrc" ]; then
            if ! grep -q "ryo2d/bin" "$HOME/.zshrc"; then
                status "Updating" ".zshrc..."
                echo "" >> "$HOME/.zshrc"
                echo "# Ryo2D CLI" >> "$HOME/.zshrc"
                echo "$export_line" >> "$HOME/.zshrc"
                updated=true
            fi
        fi
        
        status "Updating" "current session PATH..."
        # Update current session
        export PATH="$PATH:$dir"
        
        return 0
    fi
    
    return 1
}

# Uninstall mode
if [ "$UNINSTALL" = true ]; then
    echo ""
    echo "Ryo2D CLI Uninstaller"
    echo "====================="
    echo ""
    
    removed=false
    
    if [ -f "$BINARY_PATH" ]; then
        status "Removing" "ryo2d from $INSTALL_DIR"
        rm -f "$BINARY_PATH"
        removed=true
    fi
    
    # Remove install directory if empty
    if [ -d "$INSTALL_DIR" ] && [ -z "$(ls -A "$INSTALL_DIR")" ]; then
        status "Removing" "empty directory $INSTALL_DIR"
        rmdir "$INSTALL_DIR"
        # Remove parent if also empty
        if [ -d "$HOME/.ryo2d" ] && [ -z "$(ls -A "$HOME/.ryo2d")" ]; then
            rmdir "$HOME/.ryo2d"
        fi
    fi
    
    if [ "$removed" = true ]; then
        echo ""
        success "ryo2d CLI uninstalled"
        echo ""
        echo -e "\033[90mNote: PATH was not modified. You may want to manually remove:\033[0m"
        echo -e "  \033[90m$INSTALL_DIR\033[0m"
        echo -e "\033[90mfrom your shell configuration files (.bashrc, .zshrc, etc.)\033[0m"
        echo ""
    else
        warning "ryo2d CLI not found at $BINARY_PATH"
    fi
    
    exit 0
fi

# Install mode
echo ""
echo "Ryo2D CLI Installer"
echo "==================="
echo ""

# Check for Rust/Cargo
if ! command -v cargo &> /dev/null; then
    error "Cargo not found. Please install Rust from https://rustup.rs/"
    exit 1
fi

status "Building" "ryo2d CLI in release mode..."
echo ""

cd "$SCRIPT_DIR"
if ! cargo build --release --bin ryo2d; then
    error "Build failed"
    exit 1
fi

echo ""
status "Creating" "installation directory $INSTALL_DIR"
mkdir -p "$INSTALL_DIR"

source_binary="$SCRIPT_DIR/target/release/ryo2d"
status "Installing" "ryo2d to $INSTALL_DIR"
cp "$source_binary" "$BINARY_PATH"
chmod +x "$BINARY_PATH"

# Copy skills directory for install-skills command
source_skills="$SCRIPT_DIR/.github/skills"
target_skills="$HOME/.ryo2d/skills"
if [ -d "$source_skills" ]; then
    status "Installing" "Agent Skills to ~/.ryo2d/skills"
    rm -rf "$target_skills"
    cp -r "$source_skills" "$target_skills"
fi

# Install engine source for game projects to reference
engine_dir="$HOME/.ryo2d/engine"
status "Installing" "ryo2d engine source to ~/.ryo2d/engine"
rm -rf "$engine_dir"
mkdir -p "$engine_dir"

# Copy essential engine files
for item in "src" "Cargo.toml" "LICENSE" "README.md"; do
    if [ -e "$SCRIPT_DIR/$item" ]; then
        cp -r "$SCRIPT_DIR/$item" "$engine_dir/"
    fi
done

echo ""
success "ryo2d CLI installed successfully!"

# Add to PATH
if add_to_path "$INSTALL_DIR"; then
    echo ""
    echo -e "\033[32mInstallation complete! You can now use 'ryo2d' from any directory.\033[0m"
    echo ""
    echo -e "\033[90mThe PATH has been updated for:\033[0m"
    echo -e "  \033[90m- Current terminal session (works immediately)\033[0m"
    echo -e "  \033[90m- Future terminal sessions\033[0m"
else
    echo ""
    echo -e "\033[32mInstallation complete!\033[0m"
    echo ""
    if in_path "$INSTALL_DIR"; then
        echo -e "\033[90mYou can now use 'ryo2d' from any directory.\033[0m"
    else
        echo -e "\033[33mTo use 'ryo2d' from any directory, add this to your PATH:\033[0m"
        echo -e "  \033[90m$INSTALL_DIR\033[0m"
        echo ""
        echo -e "\033[90mAdd this line to your ~/.bashrc or ~/.zshrc:\033[0m"
        echo -e "  \033[90mexport PATH=\"\$PATH:$INSTALL_DIR\"\033[0m"
    fi
fi

echo ""
echo -e "\033[36mRun 'ryo2d --help' to get started.\033[0m"
echo ""

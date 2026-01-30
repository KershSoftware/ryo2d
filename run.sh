#!/usr/bin/env bash
# Ryo2D Development Run Script (Unix/Linux/macOS)
# For Windows, use run.ps1 instead
#
# Usage:
#   ./run.sh              - Interactive example selection
#   ./run.sh <example>    - Run specific example (e.g., ./run.sh breakout)
#   ./run.sh check        - Fast compilation check
#   ./run.sh lint         - Linting with clippy
#   ./run.sh test         - Run tests
#   ./run.sh watch        - Watch mode with hot-reload (requires cargo-watch)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMMAND="${1:-}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
WHITE='\033[1;37m'
GRAY='\033[0;90m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to discover examples
get_examples() {
    local examples=()
    local display_names=()
    
    if [[ -d "$SCRIPT_DIR/examples" ]]; then
        for dir in "$SCRIPT_DIR/examples"/*/; do
            if [[ -d "$dir" ]]; then
                local name=$(basename "$dir")
                local main_rs="$dir/main.rs"
                
                # Check for config file (case-insensitive)
                local ryo_toml=$(find "$dir" -maxdepth 1 -iname "ryo.toml" -type f | head -1)
                
                # Check if it's a valid example (has main.rs)
                if [[ -f "$main_rs" ]]; then
                    local display_name="$name"
                    
                    # Try to read project name from Ryo.toml
                    if [[ -n "$ryo_toml" ]]; then
                        local parsed_name=$(grep -E '^name\s*=' "$ryo_toml" | sed 's/.*=\s*"\([^"]*\)".*/\1/' | head -1)
                        if [[ -n "$parsed_name" ]]; then
                            display_name="$parsed_name"
                        fi
                    fi
                    
                    examples+=("$name")
                    display_names+=("$display_name")
                fi
            fi
        done
    fi
    
    # Export as newline-separated strings
    printf '%s\n' "${examples[@]}"
}

get_display_names() {
    if [[ -d "$SCRIPT_DIR/examples" ]]; then
        for dir in "$SCRIPT_DIR/examples"/*/; do
            if [[ -d "$dir" ]]; then
                local name=$(basename "$dir")
                local main_rs="$dir/main.rs"
                
                # Check for config file (case-insensitive)
                local ryo_toml=$(find "$dir" -maxdepth 1 -iname "ryo.toml" -type f | head -1)
                
                if [[ -f "$main_rs" ]]; then
                    local display_name="$name"
                    
                    if [[ -n "$ryo_toml" ]]; then
                        local parsed_name=$(grep -E '^name\s*=' "$ryo_toml" | sed 's/.*=\s*"\([^"]*\)".*/\1/' | head -1)
                        if [[ -n "$parsed_name" ]]; then
                            display_name="$parsed_name"
                        fi
                    fi
                    
                    echo "$display_name"
                fi
            fi
        done
    fi
}

# Function to run an example (from its own directory as a standalone project)
run_example() {
    local example_name="$1"
    local example_path="$SCRIPT_DIR/examples/$example_name"
    echo ""
    echo -e "${CYAN}Running example: $example_name${NC}"
    echo -e "${GRAY}  Path: $example_path${NC}"
    echo ""
    
    # Save current directory and move to example directory
    local original_dir="$(pwd)"
    cd "$example_path"
    
    # Run and ensure we return to original directory
    # Use cargo to run the ryo2d CLI from the engine root
    cargo run --manifest-path "$SCRIPT_DIR/Cargo.toml" --bin ryo2d -- dev || true
    cd "$original_dir"
}

# Function to show interactive example selection
show_example_menu() {
    local examples=()
    local display_names=()
    
    while IFS= read -r line; do
        examples+=("$line")
    done < <(get_examples)
    
    while IFS= read -r line; do
        display_names+=("$line")
    done < <(get_display_names)
    
    local count=${#examples[@]}
    
    if [[ $count -eq 0 ]]; then
        echo -e "${RED}No examples found in examples/ directory.${NC}"
        exit 1
    fi
    
    # Auto-select if only one example
    if [[ $count -eq 1 ]]; then
        echo -e "${GREEN}Auto-selecting only example: ${display_names[0]}${NC}"
        run_example "${examples[0]}"
        return
    fi
    
    echo ""
    echo -e "${CYAN}=== Ryo2D Examples ===${NC}"
    echo ""
    
    for ((i=0; i<count; i++)); do
        local num=$((i + 1))
        echo -e "  ${WHITE}[$num] ${display_names[$i]}${NC}"
    done
    
    echo ""
    echo -e "  ${GRAY}[q] Quit${NC}"
    echo ""
    
    read -p "Select an example: " selection
    
    if [[ "$selection" == "q" || "$selection" == "Q" ]]; then
        echo -e "${GRAY}Exiting.${NC}"
        exit 0
    fi
    
    local index=$((selection - 1))
    if [[ $index -ge 0 && $index -lt $count ]]; then
        run_example "${examples[$index]}"
    else
        echo -e "${RED}Invalid selection.${NC}"
        exit 1
    fi
}

# Execute the appropriate command
case "$COMMAND" in
    "")
        show_example_menu
        ;;
    check)
        cargo check
        ;;
    lint)
        cargo clippy
        ;;
    test)
        cargo test
        ;;
    watch)
        # Watch mode - runs first example by default
        mapfile -t examples < <(get_examples)
        if [[ ${#examples[@]} -gt 0 ]]; then
            local example_name="${examples[0]}"
            local example_path="$SCRIPT_DIR/examples/$example_name"
            echo -e "${CYAN}Watching example: $example_name (Ctrl+C to stop)${NC}"
            echo -e "${GRAY}  Path: $example_path${NC}"
            cd "$example_path"
            cargo watch -c -x run
        else
            echo -e "${RED}No examples found.${NC}"
            exit 1
        fi
        ;;
    help)
        echo ""
        echo -e "${CYAN}Ryo2D Development Script${NC}"
        echo ""
        echo "Usage:"
        echo "  ./run.sh              - Interactive example selection"
        echo "  ./run.sh <example>    - Run specific example (e.g., ./run.sh breakout)"
        echo "  ./run.sh check        - Fast compilation check"
        echo "  ./run.sh lint         - Linting with clippy"
        echo "  ./run.sh test         - Run tests"
        echo "  ./run.sh watch        - Watch mode with hot-reload"
        echo "  ./run.sh help         - Show this help"
        echo ""
        ;;
    *)
        # Try to run as an example name
        mapfile -t examples < <(get_examples)
        
        found=false
        for example in "${examples[@]}"; do
            if [[ "$example" == "$COMMAND" ]]; then
                run_example "$example"
                found=true
                break
            fi
        done
        
        if [[ "$found" == false ]]; then
            echo -e "${RED}Unknown command or example: $COMMAND${NC}"
            echo ""
            echo -e "${YELLOW}Available examples:${NC}"
            for example in "${examples[@]}"; do
                echo "  - $example"
            done
            echo ""
            echo "Run './run.sh help' for more options."
            exit 1
        fi
        ;;
esac

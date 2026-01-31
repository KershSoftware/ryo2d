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
#   ./run.sh coverage     - Generate code coverage report
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
        # Search in both demos/ and games/ subdirectories
        for category in "demos" "games"; do
            local category_path="$SCRIPT_DIR/examples/$category"
            if [[ -d "$category_path" ]]; then
                for dir in "$category_path"/*/; do
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
                            
                            # Output: category/name
                            echo "$category/$name"
                        fi
                    fi
                done
            fi
        done
    fi
}

get_display_names() {
    if [[ -d "$SCRIPT_DIR/examples" ]]; then
        # Search in both demos/ and games/ subdirectories
        for category in "demos" "games"; do
            local category_path="$SCRIPT_DIR/examples/$category"
            if [[ -d "$category_path" ]]; then
                for dir in "$category_path"/*/; do
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
        donepath_with_category="$1"
    local example_name=$(basename "$example_path_with_category")
    local example_path="$SCRIPT_DIR/examples/$example_path_with_category
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
    
    # Build and run the game
    echo -e "${YELLOW}Building...${NC}"
    if cargo run --manifest-path "$SCRIPT_DIR/Cargo.toml" --bin ryo2d -- build; then
        echo -e "${YELLOW}Running...${NC}"
        cargo run --manifest-path "$SCRIPT_DIR/Cargo.toml" --bin ryo2d -- run || true
    else
        echo -e "${RED}Build failed!${NC}"
    fi
    
    cd "$original_dir"
}

# Function to show category selection menu
show_category_menu() {
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
    
    # Separate examples into demos and games
    local demos=()
    local games=()
    local demos_display=()
    local games_display=()
    
    for ((i=0; i<count; i++)); do
        local example="${examples[$i]}"
        local display="${display_names[$i]}"
        if [[ "$example" == demos/* ]]; then
            demos+=("$example")
            demos_display+=("$display")
        else
            games+=("$example")
            games_display+=("$display")
        fi
    done
    
    # Check if we have both categories
    local has_games=$([[ ${#games[@]} -gt 0 ]] && echo "true" || echo "false")
    local has_demos=$([[ ${#demos[@]} -gt 0 ]] && echo "true" || echo "false")
    
    # If only one category exists, skip to that category
    if [[ "$has_games" == "true" && "$has_demos" == "false" ]]; then
        show_example_list games "${games[@]}" -- "${games_display[@]}"
        return
    fi
    if [[ "$has_demos" == "true" && "$has_games" == "false" ]]; then
        show_example_list demos "${demos[@]}" -- "${demos_display[@]}"
        return
    fi
    
    # Show category selection
    echo ""
    echo -e "${CYAN}=== Ryo2D Examples ===${NC}"
    echo ""
    echo -e "  ${WHITE}[1] Games${NC}"
    echo -e "  ${WHITE}[2] Demos${NC}"
    echo ""
    echo -e "  ${GRAY}[q] Quit${NC}"
    echo ""
    
    echo -n "Select a category: "
    read -n 1 -r selection
    echo ""
    echo ""
    
    case "$selection" in
        1)
            show_example_list games "${games[@]}" -- "${games_display[@]}"
            ;;
        2)
            show_example_list demos "${demos[@]}" -- "${demos_display[@]}"
            ;;
        *)
            echo -e "${GRAY}Exiting.${NC}"
            exit 0
            ;;
    esac
}

# Function to show examples within a category
show_example_list() {
    local category_name="$1"
    shift
    
    local category_examples=()
    local category_display=()
    local reading_examples=true
    
    for arg in "$@"; do
        if [[ "$arg" == "--" ]]; then
            reading_examples=false
            continue
        fi
        if [[ "$reading_examples" == "true" ]]; then
            category_examples+=("$arg")
        else
            category_display+=("$arg")
        fi
    done
    
    local count=${#category_examples[@]}
    
    if [[ $count -eq 0 ]]; then
        echo -e "${RED}No examples found in $category_name.${NC}"
        exit 1
    fi
    
    # Auto-select if only one example
    if [[ $count -eq 1 ]]; then
        echo -e "${GREEN}Auto-selecting only example: ${category_display[0]}${NC}"
        run_example "${category_examples[0]}"
        return
    fi
    
    # Capitalize category name for display
    local display_name="$(tr '[:lower:]' '[:upper:]' <<< ${category_name:0:1})${category_name:1}"
    
    echo ""
    echo -e "${CYAN}=== $display_name ===${NC}"
    echo ""
    
    for ((i=0; i<count; i++)); do
        local num=$((i + 1))
        echo -e "  ${WHITE}[$num] ${category_display[$i]}${NC}"
    done
    
    echo ""
    echo -e "  ${GRAY}[b] Back${NC}"
    echo -e "  ${GRAY}[q] Quit${NC}"
    echo ""
    
    echo -n "Select an example: "
    read -n 1 -r selection
    echo ""
    echo ""
    
    case "$selection" in
        b|B)
            show_category_menu
            return
            ;;
        *)
            # Try to use as number
            if [[ "$selection" =~ ^[0-9]+$ ]]; then
                local index=$((selection - 1))
                if [[ $index -ge 0 && $index -lt $count ]]; then
                    run_example "${category_examples[$index]}"
                    return
                fi
            fi
            # Any other input (including 'q' or invalid) quits
            echo -e "${GRAY}Exiting.${NC}"
            exit 0
            ;;
    esac
}

# Execute the appropriate command
case "$COMMAND" in
    "")
        show_category_menu
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
    coverage)
        echo -e "${CYAN}Generating code coverage report...${NC}"
        echo ""
        
        # Check if cargo-llvm-cov is installed
        if ! command -v cargo-llvm-cov &> /dev/null; then
            echo -e "${YELLOW}cargo-llvm-cov is not installed.${NC}"
            echo -e "${CYAN}Installing cargo-llvm-cov...${NC}"
            cargo install cargo-llvm-cov
            echo ""
        fi
        
        # Generate coverage report (HTML format, library only, excluding CLI)
        cargo llvm-cov --lib --ignore-filename-regex="cli" --html --open
        
        if [[ $? -eq 0 ]]; then
            echo ""
            echo -e "${GREEN}Coverage report generated successfully!${NC}"
            echo -e "${GRAY}Report opened in browser. Also available at: target/llvm-cov/html/index.html${NC}"
        fi
        ;;
    watch)
        # Wa# Match both "demos/example" and just "example"
            local base_name=$(basename "$example")
            if [[ "$example" == "$COMMAND" || "$base_name" == "$COMMAND" ]]; then
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
                echo "  - $(basename "$example")
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
        echo "  ./run.sh coverage     - Generate code coverage report"
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

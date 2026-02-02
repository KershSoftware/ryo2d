#!/usr/bin/env bash
# Ryo2D Development Run Script (Unix/Linux/macOS)
# For Windows, use run.ps1 instead
#
# Usage:
#     ./run.sh                     - Interactive example selection
#     ./run.sh <example>           - Run specific example (shows mode menu)
#     ./run.sh <example> --dev     - Run in dev mode with hot-reload
#     ./run.sh --dev <example>     - Same as above
#     ./run.sh check               - Fast compilation check (engine only)
#     ./run.sh check-all           - Check + lint all examples in parallel
#     ./run.sh lint                - Linting with clippy (engine only)
#     ./run.sh test                - Run unit + integration tests
#     ./run.sh test:unit           - Run unit tests only
#     ./run.sh test:integration    - Run integration tests only
#     ./run.sh test:e2e            - Run e2e tests (requires GPU)
#     ./run.sh coverage            - Generate code coverage report

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Add cargo to PATH if not already present
CARGO_PATH="$HOME/.cargo/bin"
if [[ ":$PATH:" != *":$CARGO_PATH:"* ]]; then
    export PATH="$CARGO_PATH:$PATH"
fi

# Parse arguments
DEV_MODE=false
COMMAND=""

for arg in "$@"; do
    if [[ "$arg" == "--dev" || "$arg" == "-d" ]]; then
        DEV_MODE=true
    elif [[ -z "$COMMAND" ]]; then
        COMMAND="$arg"
    fi
done

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
WHITE='\033[1;37m'
GRAY='\033[0;90m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to discover examples
get_examples() {
    if [[ -d "$SCRIPT_DIR/examples" ]]; then
        for category in "demos" "games"; do
            local category_path="$SCRIPT_DIR/examples/$category"
            if [[ -d "$category_path" ]]; then
                for dir in "$category_path"/*/; do
                    if [[ -d "$dir" ]]; then
                        local name=$(basename "$dir")
                        local main_rs="$dir/main.rs"
                        local src_main_rs="$dir/src/main.rs"

                        if [[ -f "$main_rs" || -f "$src_main_rs" ]]; then
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
        for category in "demos" "games"; do
            local category_path="$SCRIPT_DIR/examples/$category"
            if [[ -d "$category_path" ]]; then
                for dir in "$category_path"/*/; do
                    if [[ -d "$dir" ]]; then
                        local name=$(basename "$dir")
                        local main_rs="$dir/main.rs"
                        local src_main_rs="$dir/src/main.rs"
                        local ryo_toml=$(find "$dir" -maxdepth 1 -iname "ryo.toml" -type f | head -1)

                        if [[ -f "$main_rs" || -f "$src_main_rs" ]]; then
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
        done
    fi
}

# Function to show run mode menu
show_run_mode_menu() {
    echo ""
    echo -e "    ${WHITE}[1] Build + Run (release)${NC}"
    echo -e "    ${WHITE}[2] Dev mode (hot-reload)${NC}"
    echo ""
    echo -n "Select mode [1]: "
    read -n 1 -r selection
    echo ""

    if [[ "$selection" == "2" ]]; then
        return 0  # true = dev mode
    else
        return 1  # false = build+run mode
    fi
}

# Function to run an example
run_example() {
    local example_name="$1"
    local use_dev_mode="$2"
    local ask_mode="$3"
    local example_path="$SCRIPT_DIR/examples/$example_name"
    local base_name=$(basename "$example_name")

    # If ask_mode is true, prompt for mode selection
    if [[ "$ask_mode" == "true" ]]; then
        if show_run_mode_menu; then
            use_dev_mode="true"
        else
            use_dev_mode="false"
        fi
    fi

    echo ""
    echo -e "${CYAN}Running example: $example_name${NC}"
    echo -e "${GRAY}  Path: $example_path${NC}"

    local original_dir="$(pwd)"
    cd "$example_path"

    if [[ "$use_dev_mode" == "true" ]]; then
        echo -e "${MAGENTA}  Mode: Development (hot-reload enabled)${NC}"
        echo ""
        cargo run --manifest-path "$SCRIPT_DIR/Cargo.toml" --bin ryo2d -- dev || true
    else
        echo -e "${GREEN}  Mode: Build + Run (release)${NC}"
        echo ""
        echo -e "${YELLOW}Building...${NC}"
        if cargo run --manifest-path "$SCRIPT_DIR/Cargo.toml" --bin ryo2d -- build; then
            echo -e "${YELLOW}Running...${NC}"
            cargo run --manifest-path "$SCRIPT_DIR/Cargo.toml" --bin ryo2d -- run || true
        else
            echo -e "${RED}Build failed!${NC}"
        fi
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
    echo -e "    ${WHITE}[1] Games${NC}"
    echo -e "    ${WHITE}[2] Demos${NC}"
    echo ""
    echo -e "    ${GRAY}[q] Quit${NC}"
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
        run_example "${category_examples[0]}" "false" "true"
        return
    fi

    # Capitalize category name for display
    local display_name="$(tr '[:lower:]' '[:upper:]' <<< ${category_name:0:1})${category_name:1}"

    echo ""
    echo -e "${CYAN}=== $display_name ===${NC}"
    echo ""

    for ((i=0; i<count; i++)); do
        local num=$((i + 1))
        echo -e "    ${WHITE}[$num] ${category_display[$i]}${NC}"
    done

    echo ""
    echo -e "    ${GRAY}[b] Back${NC}"
    echo -e "    ${GRAY}[q] Quit${NC}"
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
            if [[ "$selection" =~ ^[0-9]+$ ]]; then
                local index=$((selection - 1))
                if [[ $index -ge 0 && $index -lt $count ]]; then
                    run_example "${category_examples[$index]}" "false" "true"
                    return
                fi
            fi
            echo -e "${GRAY}Exiting.${NC}"
            exit 0
            ;;
    esac
}

# Function to check all examples in parallel
check_all_examples() {
    echo ""
    echo -e "${CYAN}=== Checking All Examples ===${NC}"
    echo ""

    mapfile -t examples < <(get_examples)
    mapfile -t display_names < <(get_display_names)

    local count=${#examples[@]}

    if [[ $count -eq 0 ]]; then
        echo -e "${RED}No examples found.${NC}"
        exit 1
    fi

    echo -e "${GRAY}Found $count examples + engine workspace${NC}"
    echo -e "${GRAY}Running cargo check and clippy in parallel...${NC}"
    echo ""

    # Create temp directory for output
    local temp_dir=$(mktemp -d)
    trap "rm -rf $temp_dir" EXIT

    # Array to track background jobs
    local pids=()
    local targets=()

    # Start engine check first
    {
        cargo check --manifest-path "$SCRIPT_DIR/Cargo.toml" > "$temp_dir/engine_check.out" 2>&1
        echo $? > "$temp_dir/engine_check.status"

        cargo clippy --manifest-path "$SCRIPT_DIR/Cargo.toml" -- -D warnings > "$temp_dir/engine_lint.out" 2>&1
        echo $? > "$temp_dir/engine_lint.status"
    } &
    pids+=($!)
    targets+=("engine")
    echo -e "  ${GRAY}Started: Engine (workspace)${NC}"

    # Start checks for each example
    for ((i=0; i<count; i++)); do
        local example="${examples[$i]}"
        local display="${display_names[$i]}"
        local example_path="$SCRIPT_DIR/examples/$example"
        local safe_name=$(echo "$example" | tr '/' '_')

        {
            cargo check --manifest-path "$example_path/Cargo.toml" > "$temp_dir/${safe_name}_check.out" 2>&1
            echo $? > "$temp_dir/${safe_name}_check.status"

            cargo clippy --manifest-path "$example_path/Cargo.toml" -- -D warnings > "$temp_dir/${safe_name}_lint.out" 2>&1
            echo $? > "$temp_dir/${safe_name}_lint.status"
        } &
        pids+=($!)
        targets+=("$example")
        echo -e "  ${GRAY}Started: $display${NC}"
    done

    echo ""
    echo -e "${GRAY}Waiting for all jobs to complete...${NC}"

    # Wait for all jobs
    for pid in "${pids[@]}"; do
        wait $pid 2>/dev/null || true
    done

    # Collect and display results
    echo ""
    echo -e "${CYAN}=== Summary ===${NC}"
    echo ""
    printf "${WHITE}%-35s %-10s %-10s${NC}\n" "Project" "Check" "Clippy"
    printf "${GRAY}%-35s %-10s %-10s${NC}\n" "-------" "-----" "------"

    local failed_checks=()
    local failed_lints=()
    local all_passed=true

    # Engine results
    local engine_check_status=$(cat "$temp_dir/engine_check.status" 2>/dev/null || echo "1")
    local engine_lint_status=$(cat "$temp_dir/engine_lint.status" 2>/dev/null || echo "1")

    if [[ "$engine_check_status" -eq 0 ]]; then
        local check_result="${GREEN}PASS${NC}"
    else
        local check_result="${RED}FAIL${NC}"
        failed_checks+=("Engine (workspace)|$temp_dir/engine_check.out")
        all_passed=false
    fi

    if [[ "$engine_lint_status" -eq 0 ]]; then
        local lint_result="${GREEN}PASS${NC}"
    else
        local lint_result="${YELLOW}FAIL${NC}"
        failed_lints+=("Engine (workspace)|$temp_dir/engine_lint.out")
        all_passed=false
    fi

    printf "%-35s " "Engine (workspace)"
    echo -e "$check_result       $lint_result"

    # Example results
    for ((i=0; i<count; i++)); do
        local example="${examples[$i]}"
        local display="${display_names[$i]}"
        local safe_name=$(echo "$example" | tr '/' '_')

        local check_status=$(cat "$temp_dir/${safe_name}_check.status" 2>/dev/null || echo "1")
        local lint_status=$(cat "$temp_dir/${safe_name}_lint.status" 2>/dev/null || echo "1")

        if [[ "$check_status" -eq 0 ]]; then
            local check_result="${GREEN}PASS${NC}"
        else
            local check_result="${RED}FAIL${NC}"
            failed_checks+=("$display|$temp_dir/${safe_name}_check.out")
            all_passed=false
        fi

        if [[ "$lint_status" -eq 0 ]]; then
            local lint_result="${GREEN}PASS${NC}"
        else
            local lint_result="${YELLOW}FAIL${NC}"
            failed_lints+=("$display|$temp_dir/${safe_name}_lint.out")
            all_passed=false
        fi

        printf "%-35s " "$display"
        echo -e "$check_result       $lint_result"
    done

    # Show errors if any
    if [[ ${#failed_checks[@]} -gt 0 || ${#failed_lints[@]} -gt 0 ]]; then
        echo ""
        echo -e "${RED}=== Errors ===${NC}"

        for fail in "${failed_checks[@]}"; do
            local name="${fail%%|*}"
            local file="${fail##*|}"
            echo ""
            echo -e "${RED}[$name] Compilation errors:${NC}"
            echo -e "${GRAY}$(cat "$file")${NC}"
        done

        for fail in "${failed_lints[@]}"; do
            local name="${fail%%|*}"
            local file="${fail##*|}"
            echo ""
            echo -e "${YELLOW}[$name] Clippy warnings/errors:${NC}"
            echo -e "${GRAY}$(cat "$file")${NC}"
        done

        echo ""
        local total_failed=$((${#failed_checks[@]} + ${#failed_lints[@]}))
        echo -e "${RED}Check completed with $total_failed failure(s).${NC}"
        exit 1
    else
        echo ""
        echo -e "${GREEN}All checks passed!${NC}"
    fi
}

# Execute the appropriate command
case "$COMMAND" in
    "")
        show_category_menu
        ;;
    check)
        cargo check
        ;;
    check-all)
        check_all_examples
        ;;
    lint)
        cargo clippy
        ;;
    test)
        # Default: run unit tests + integration tests
        echo -e "${CYAN}Running unit + integration tests...${NC}"
        cargo test --lib --test integration
        ;;
    test:unit)
        echo -e "${CYAN}Running unit tests only...${NC}"
        cargo test --lib
        ;;
    test:integration)
        echo -e "${CYAN}Running integration tests only...${NC}"
        cargo test --test integration
        ;;
    test:e2e)
        echo -e "${CYAN}Running e2e tests (includes ignored tests requiring GPU)...${NC}"
        cargo test --test e2e -- --include-ignored
        ;;
    test:all)
        echo -e "${CYAN}Running ALL tests (unit + integration + e2e)...${NC}"
        cargo test --lib --test integration --test e2e -- --include-ignored
        ;;
    coverage)
        echo -e "${CYAN}Generating code coverage report...${NC}"
        echo ""

        if ! command -v cargo-llvm-cov &> /dev/null; then
            echo -e "${YELLOW}cargo-llvm-cov is not installed.${NC}"
            echo -e "${CYAN}Installing cargo-llvm-cov...${NC}"
            cargo install cargo-llvm-cov
            echo ""
        fi

        cargo llvm-cov --lib --ignore-filename-regex="cli" --html --open

        if [[ $? -eq 0 ]]; then
            echo ""
            echo -e "${GREEN}Coverage report generated successfully!${NC}"
            echo -e "${GRAY}Report opened in browser. Also available at: target/llvm-cov/html/index.html${NC}"
        fi
        ;;
    watch)
        if ! command -v cargo-watch &> /dev/null; then
            echo -e "${YELLOW}cargo-watch is not installed.${NC}"
            echo -e "${CYAN}Installing cargo-watch...${NC}"
            cargo install cargo-watch
            echo ""
        fi

        echo -e "${CYAN}Starting watch mode...${NC}"
        cargo watch -x check -x test
        ;;
    help)
        echo ""
        echo -e "${CYAN}Ryo2D Development Script${NC}"
        echo ""
        echo "Usage:"
        echo "    ./run.sh                     - Interactive example selection"
        echo "    ./run.sh <example>           - Run specific example (shows mode menu)"
        echo "    ./run.sh <example> --dev     - Run in dev mode with hot-reload"
        echo "    ./run.sh --dev <example>     - Same as above"
        echo "    ./run.sh check               - Fast compilation check (engine only)"
        echo "    ./run.sh check-all           - Check + lint all examples in parallel"
        echo "    ./run.sh lint                - Linting with clippy (engine only)"
        echo "    ./run.sh test                - Run unit + integration tests (default)"
        echo "    ./run.sh test:unit           - Run unit tests only"
        echo "    ./run.sh test:integration    - Run integration tests only"
        echo "    ./run.sh test:e2e            - Run e2e tests (requires GPU)"
        echo "    ./run.sh test:all            - Run ALL tests"
        echo "    ./run.sh coverage            - Generate code coverage report"
        echo "    ./run.sh watch               - Watch mode with hot-reload"
        echo "    ./run.sh help                - Show this help"
        echo ""
        ;;
    *)
        # Try to run as an example name
        mapfile -t examples < <(get_examples)

        found=false
        for example in "${examples[@]}"; do
            # Match by full path (demos/example or games/example) or just the name
            base=$(basename "$example")
            if [[ "$example" == "$COMMAND" || "$base" == "$COMMAND" ]]; then
                if [[ "$DEV_MODE" == "true" ]]; then
                    run_example "$example" "true" "false"
                else
                    run_example "$example" "false" "true"
                fi
                found=true
                break
            fi
        done

        if [[ "$found" == false ]]; then
            echo -e "${RED}Unknown command or example: $COMMAND${NC}"
            echo ""
            echo -e "${YELLOW}Available examples:${NC}"
            for example in "${examples[@]}"; do
                echo "    - $example"
            done
            echo ""
            echo "Run './run.sh help' for more options."
            exit 1
        fi
        ;;
esac

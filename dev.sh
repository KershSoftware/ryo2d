#!/usr/bin/env bash
# Ryo2D Development Script (Unix/Linux/macOS)
# For Windows, use dev.ps1 instead
#
# Usage:
#     ./dev.sh                         - Interactive example selection
#     ./dev.sh <example>               - Run specific example (shows mode menu)
#     ./dev.sh <example> -d|--dev      - Run in dev mode with hot-reload
#     ./dev.sh <example> -p|--profiler - Run in dev mode with profiler window
#     ./dev.sh <example> -r|--release  - Build and run release (no prompts)
#     ./dev.sh -d|--dev <example>      - Same as above (flag before example)
#     ./dev.sh -p|--profiler <example> - Same as above (flag before example)
#     ./dev.sh -r|--release <example>  - Same as above (flag before example)
#     ./dev.sh demos:run-all           - Launch all demos in dev mode (parallel terminals)
#     ./dev.sh demos:kill-all          - Kill all running demos
#     ./dev.sh demos:tour [demo...]    - Capture every page of every demo headlessly (target/demo-tour)
#     ./dev.sh check                   - Compilation check (engine + all examples, engine for wasm)
#     ./dev.sh lint                    - Linting with clippy (engine + all examples, incl. tests; engine for wasm)
#     ./dev.sh test                    - Run unit + integration tests
#     ./dev.sh test:unit               - Run unit tests only
#     ./dev.sh test:integration        - Run integration tests only
#     ./dev.sh test:e2e                - Run e2e tests (requires GPU)
#     ./dev.sh coverage                - Unit test coverage (default)
#     ./dev.sh coverage:unit           - Unit test coverage
#     ./dev.sh coverage:integration    - Integration test coverage
#     ./dev.sh coverage:all            - Combined coverage (unit + integration)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Add cargo to PATH if not already present
CARGO_PATH="$HOME/.cargo/bin"
if [[ ":$PATH:" != *":$CARGO_PATH:"* ]]; then
    export PATH="$CARGO_PATH:$PATH"
fi

# Parse arguments
DEV_MODE=false
RELEASE_MODE=false
PROFILER_MODE=false
COMMAND=""

for arg in "$@"; do
    if [[ "$arg" == "--dev" || "$arg" == "-d" ]]; then
        DEV_MODE=true
    elif [[ "$arg" == "--release" || "$arg" == "-r" ]]; then
        RELEASE_MODE=true
    elif [[ "$arg" == "-p" || "$arg" == "--profiler" ]]; then
        PROFILER_MODE=true
        DEV_MODE=true  # Profiler implies dev mode
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
# Returns: 0=release, 1=dev, 2=dev with profiler (via RUN_MODE variable)
show_run_mode_menu() {
    echo ""
    echo -e "    ${WHITE}[1] Dev mode (hot-reload)${NC}"
    echo -e "    ${MAGENTA}[2] Dev mode with profiler${NC}"
    echo -e "    ${WHITE}[3] Build + Run (release)${NC}"
    echo ""
    echo -n "Select mode [1]: "
    read -n 1 -r selection
    echo ""

    case "$selection" in
        2)
            RUN_MODE=2  # dev with profiler
            ;;
        3)
            RUN_MODE=0  # release
            ;;
        *)
            RUN_MODE=1  # dev mode (default)
            ;;
    esac
}

# Function to run an example
# $1 = example_name
# $2 = run_mode (0=release, 1=dev, 2=dev+profiler) or "ask" to prompt
run_example() {
    local example_name="$1"
    local run_mode="$2"
    local example_path="$SCRIPT_DIR/examples/$example_name"
    local base_name=$(basename "$example_name")

    # If run_mode is "ask", prompt for mode selection
    if [[ "$run_mode" == "ask" ]]; then
        show_run_mode_menu
        run_mode=$RUN_MODE
    fi

    echo ""
    echo -e "${CYAN}Running example: $example_name${NC}"
    echo -e "${GRAY}  Path: $example_path${NC}"

    local original_dir="$(pwd)"
    cd "$example_path"

    case "$run_mode" in
        1)
            # Dev mode
            echo -e "${MAGENTA}  Mode: Development (hot-reload enabled)${NC}"
            echo ""
            cargo run --manifest-path "$SCRIPT_DIR/Cargo.toml" --bin ryo2d -- dev --skip-opt-check || true
            ;;
        2)
            # Dev mode with profiler
            echo -e "${MAGENTA}  Mode: Development with profiler${NC}"
            echo -e "${GRAY}        A separate profiler window will open with real-time flamegraphs.${NC}"
            echo ""
            cargo run --manifest-path "$SCRIPT_DIR/Cargo.toml" --bin ryo2d -- dev --skip-opt-check -p || true
            ;;
        *)
            # Release mode (default)
            echo -e "${GREEN}  Mode: Build + Run (release)${NC}"
            echo ""
            echo -e "${YELLOW}Building...${NC}"
            if cargo run --manifest-path "$SCRIPT_DIR/Cargo.toml" --bin ryo2d -- build; then
                echo -e "${YELLOW}Running...${NC}"
                cargo run --manifest-path "$SCRIPT_DIR/Cargo.toml" --bin ryo2d -- run || true
            else
                echo -e "${RED}Build failed!${NC}"
            fi
            ;;
    esac

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
        local auto_base=$(basename "${category_examples[0]}")
        # Special case: profile-and-log always runs with profiler enabled
        if [[ "$auto_base" == "profile-and-log" ]]; then
            echo -e "${MAGENTA}Auto-enabling profiler mode for profile-and-log demo${NC}"
            run_example "${category_examples[0]}" 2  # dev with profiler
        else
            run_example "${category_examples[0]}" "ask"
        fi
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
    read -r selection
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
                    local selected_example="${category_examples[$index]}"
                    local selected_base=$(basename "$selected_example")
                    # Special case: profile-and-log always runs with profiler enabled
                    if [[ "$selected_base" == "profile-and-log" ]]; then
                        echo -e "${MAGENTA}Auto-enabling profiler mode for profile-and-log demo${NC}"
                        run_example "$selected_example" 2  # dev with profiler
                    else
                        run_example "$selected_example" "ask"
                    fi
                    return
                fi
                echo -e "${RED}Invalid selection: $selection (valid: 1-$count)${NC}"
                exit 1
            fi
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
    demos:run-all)
        PID_FILE="$SCRIPT_DIR/.demo-pids"

        # Kill any existing demos first
        if [[ -f "$PID_FILE" ]]; then
            while read -r pid; do
                kill "$pid" 2>/dev/null || true
            done < "$PID_FILE"
            rm -f "$PID_FILE"
        fi

        # Get all demo package names and paths
        DEMO_NAMES=()
        DEMO_PATHS=()
        for toml in "$SCRIPT_DIR"/examples/demos/*/Cargo.toml; do
            if [[ -f "$toml" ]]; then
                name=$(grep -m1 '^name = ' "$toml" | sed 's/name = "\(.*\)"/\1/')
                if [[ -n "$name" ]]; then
                    DEMO_NAMES+=("$name")
                    DEMO_PATHS+=("$(dirname "$toml")")
                fi
            fi
        done

        if [[ ${#DEMO_NAMES[@]} -eq 0 ]]; then
            echo -e "${YELLOW}No demos found in examples/demos/${NC}"
            exit 1
        fi

        echo -e "${CYAN}Launching ${#DEMO_NAMES[@]} demos in dev mode...${NC}"
        > "$PID_FILE"

        # Detect available terminal emulator
        TERM_CMD=""
        if command -v gnome-terminal &> /dev/null; then
            TERM_CMD="gnome-terminal"
        elif command -v konsole &> /dev/null; then
            TERM_CMD="konsole"
        elif command -v xterm &> /dev/null; then
            TERM_CMD="xterm"
        elif [[ "$(uname)" == "Darwin" ]]; then
            TERM_CMD="osascript"  # macOS
        fi

        for i in "${!DEMO_NAMES[@]}"; do
            demo="${DEMO_NAMES[$i]}"
            demo_path="${DEMO_PATHS[$i]}"
            ryo2d_cmd="cargo run --manifest-path \"$SCRIPT_DIR/Cargo.toml\" --bin ryo2d -- dev --skip-opt-check"

            if [[ "$TERM_CMD" == "gnome-terminal" ]]; then
                gnome-terminal --working-directory="$demo_path" -- bash -c "$ryo2d_cmd; exec bash" &
            elif [[ "$TERM_CMD" == "konsole" ]]; then
                konsole --workdir "$demo_path" -e bash -c "$ryo2d_cmd; exec bash" &
            elif [[ "$TERM_CMD" == "xterm" ]]; then
                xterm -e "cd '$demo_path' && $ryo2d_cmd; exec bash" &
            elif [[ "$TERM_CMD" == "osascript" ]]; then
                # macOS: use AppleScript to open Terminal.app
                osascript -e "tell application \"Terminal\" to do script \"cd '$demo_path' && $ryo2d_cmd\"" &
            else
                # Fallback: run in background without terminal
                echo -e "  ${YELLOW}No terminal emulator found, running in background${NC}"
                (cd "$demo_path" && eval "$ryo2d_cmd" &)
            fi

            echo $! >> "$PID_FILE"
            echo -e "  ${GREEN}Started $demo in dev mode${NC}"
        done

        echo ""
        echo -e "${CYAN}All demos launched in dev mode. Run './dev.sh demos:kill-all' to close them.${NC}"
        ;;
    demos:kill-all)
        PID_FILE="$SCRIPT_DIR/.demo-pids"

        if [[ ! -f "$PID_FILE" ]]; then
            echo -e "${YELLOW}No running demos found.${NC}"
            exit 0
        fi

        echo -e "${CYAN}Killing demos...${NC}"
        killed=0
        while read -r pid; do
            if kill -0 "$pid" 2>/dev/null; then
                kill "$pid" 2>/dev/null
                echo -e "  ${GREEN}Killed PID $pid${NC}"
                ((killed++))
            fi
        done < "$PID_FILE"
        rm -f "$PID_FILE"

        if [[ $killed -gt 0 ]]; then
            echo -e "${CYAN}Done. $killed demos terminated.${NC}"
        else
            echo -e "${YELLOW}No demos were running.${NC}"
        fi
        ;;
    demos:tour)
        TOUR_ARGS=()
        for arg in "$@"; do
            [[ "$arg" == "demos:tour" ]] && continue
            if [[ "$arg" == -* ]]; then
                echo -e "${YELLOW}demos:tour takes demo names only; ignoring $arg${NC}"
            else
                TOUR_ARGS+=("$arg")
            fi
        done
        cargo run -q -p demo-kit --bin demo-tour -- "${TOUR_ARGS[@]}"
        ;;
    check)
        cargo check --workspace
        cargo check -p ryo2d --lib --target wasm32-unknown-unknown
        ;;
    lint)
        cargo clippy --workspace --all-targets
        cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown
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
    coverage|coverage:unit)
        echo -e "${CYAN}Generating unit test coverage report...${NC}"
        echo ""

        if ! command -v cargo-llvm-cov &> /dev/null; then
            echo -e "${YELLOW}cargo-llvm-cov is not installed.${NC}"
            echo -e "${CYAN}Installing cargo-llvm-cov...${NC}"
            cargo install cargo-llvm-cov
            echo ""
        fi

        cargo llvm-cov --lib --ignore-filename-regex="cli" --html --output-dir target/llvm-cov/unit --open

        if [[ $? -eq 0 ]]; then
            echo ""
            echo -e "${GREEN}Unit coverage report generated successfully!${NC}"
            echo -e "${GRAY}Report: target/llvm-cov/unit/index.html${NC}"
        fi
        ;;
    coverage:integration)
        echo -e "${CYAN}Generating integration test coverage report...${NC}"
        echo ""

        if ! command -v cargo-llvm-cov &> /dev/null; then
            echo -e "${YELLOW}cargo-llvm-cov is not installed.${NC}"
            echo -e "${CYAN}Installing cargo-llvm-cov...${NC}"
            cargo install cargo-llvm-cov
            echo ""
        fi

        cargo llvm-cov --test integration --ignore-filename-regex="cli" --html --output-dir target/llvm-cov/integration --open

        if [[ $? -eq 0 ]]; then
            echo ""
            echo -e "${GREEN}Integration coverage report generated successfully!${NC}"
            echo -e "${GRAY}Report: target/llvm-cov/integration/index.html${NC}"
        fi
        ;;
    coverage:all)
        echo -e "${CYAN}Generating combined coverage report (unit + integration)...${NC}"
        echo ""

        if ! command -v cargo-llvm-cov &> /dev/null; then
            echo -e "${YELLOW}cargo-llvm-cov is not installed.${NC}"
            echo -e "${CYAN}Installing cargo-llvm-cov...${NC}"
            cargo install cargo-llvm-cov
            echo ""
        fi

        cargo llvm-cov --lib --test integration --ignore-filename-regex="cli" --html --output-dir target/llvm-cov/all --open

        if [[ $? -eq 0 ]]; then
            echo ""
            echo -e "${GREEN}Combined coverage report generated successfully!${NC}"
            echo -e "${GRAY}Report: target/llvm-cov/all/index.html${NC}"
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
        echo "    ./dev.sh                         - Interactive example selection"
        echo "    ./dev.sh <example>               - Run specific example (shows mode menu)"
        echo "    ./dev.sh <example> -d|--dev      - Run in dev mode with hot-reload"
        echo "    ./dev.sh <example> -p|--profiler - Run in dev mode with profiler window"
        echo "    ./dev.sh <example> -r|--release  - Build and run release (no prompts)"
        echo "    ./dev.sh -d|--dev <example>      - Same as above (flag before example)"
        echo "    ./dev.sh -p|--profiler <example> - Same as above (flag before example)"
        echo "    ./dev.sh -r|--release <example>  - Same as above (flag before example)"
        echo "    ./dev.sh demos:run-all           - Launch all demos in dev mode (parallel terminals)"
        echo "    ./dev.sh demos:kill-all          - Kill all running demos"
        echo "    ./dev.sh demos:tour [demo...]    - Capture every page of every demo headlessly (target/demo-tour)"
        echo "    ./dev.sh check                   - Compilation check (engine + all examples, engine for wasm)"
        echo "    ./dev.sh lint                    - Linting with clippy (engine + all examples, incl. tests; engine for wasm)"
        echo "    ./dev.sh test                    - Run unit + integration tests (default)"
        echo "    ./dev.sh test:unit               - Run unit tests only"
        echo "    ./dev.sh test:integration        - Run integration tests only"
        echo "    ./dev.sh test:e2e                - Run e2e tests (requires GPU)"
        echo "    ./dev.sh test:all                - Run ALL tests"
        echo "    ./dev.sh coverage                - Unit test coverage (default)"
        echo "    ./dev.sh coverage:unit           - Unit test coverage"
        echo "    ./dev.sh coverage:integration    - Integration test coverage"
        echo "    ./dev.sh coverage:all            - Combined coverage (unit + integration)"
        echo "    ./dev.sh watch                   - Watch mode with hot-reload"
        echo "    ./dev.sh help                    - Show this help"
        echo ""
        ;;
    *)
        # Try to run as an example name
        examples=()
        while IFS= read -r line; do
            examples+=("$line")
        done < <(get_examples)

        found=false
        for example in "${examples[@]}"; do
            # Match by full path (demos/example or games/example) or just the name
            base=$(basename "$example")
            if [[ "$example" == "$COMMAND" || "$base" == "$COMMAND" ]]; then
                # Special case: profile-and-log always runs with profiler enabled
                if [[ "$base" == "profile-and-log" ]]; then
                    echo -e "${MAGENTA}Auto-enabling profiler mode for profile-and-log demo${NC}"
                    run_example "$example" 2  # dev with profiler
                elif [[ "$PROFILER_MODE" == "true" ]]; then
                    run_example "$example" 2  # dev with profiler
                elif [[ "$DEV_MODE" == "true" ]]; then
                    run_example "$example" 1  # dev mode
                elif [[ "$RELEASE_MODE" == "true" ]]; then
                    run_example "$example" 0  # release mode
                else
                    run_example "$example" "ask"  # prompt for mode
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
            echo "Run './dev.sh help' for more options."
            exit 1
        fi
        ;;
esac

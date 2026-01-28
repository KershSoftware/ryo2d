#!/usr/bin/env bash
# Ferrum2D Development Run Script (Unix/Linux/macOS)
# For Windows, use run.ps1 instead
#
# Usage:
#   ./run.sh          - Run the engine with hot-reload
#   ./run.sh check    - Fast compilation check with hot-reload
#   ./run.sh lint     - Linting with hot-reload
#   ./run.sh test     - Run tests with hot-reload

set -e

COMMAND="${1:-run}"

# Execute the appropriate cargo command
case "$COMMAND" in
    run)
        cargo watch -c -x run
        ;;
    check)
        cargo watch -c -x check
        ;;
    lint)
        cargo watch -c -x clippy
        ;;
    test)
        cargo watch -c -x test
        ;;
    *)
        echo "Unknown command: $COMMAND"
        echo ""
        echo "Usage:"
        echo "  ./run.sh          - Run the engine with hot-reload"
        echo "  ./run.sh check    - Fast compilation check with hot-reload"
        echo "  ./run.sh lint     - Linting with hot-reload"
        echo "  ./run.sh test     - Run tests with hot-reload"
        exit 1
        ;;
esac

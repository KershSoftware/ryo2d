#!/usr/bin/env pwsh
# Cross-platform unified run script for Ferrum2D
# Works on Windows, Linux, and macOS with PowerShell Core
#
# Usage:
#   ./run          - Run the engine with hot-reload
#   ./run check    - Fast compilation check with hot-reload
#   ./run lint     - Linting with hot-reload

param(
    [Parameter(Position = 0)]
    [string]$Command = "run"
)

# Add cargo to PATH if not already present
if ($IsWindows -or $env:OS -eq "Windows_NT") {
    $cargoPath = "$env:USERPROFILE\.cargo\bin"
} else {
    $cargoPath = "$HOME/.cargo/bin"
}

if (-not ($env:PATH -split [IO.Path]::PathSeparator -contains $cargoPath)) {
    $env:PATH = "$cargoPath$([IO.Path]::PathSeparator)$env:PATH"
}

# Execute the appropriate cargo command
switch ($Command.ToLower()) {
    "run" {
        cargo watch -c -x run
    }
    "check" {
        cargo watch -c -x check
    }
    "lint" {
        cargo watch -c -x clippy
    }
    default {
        Write-Host "Unknown command: $Command"
        Write-Host ""
        Write-Host "Usage:"
        Write-Host "  ./run          - Run the engine with hot-reload"
        Write-Host "  ./run check    - Fast compilation check with hot-reload"
        Write-Host "  ./run lint     - Linting with hot-reload"
        exit 1
    }
}

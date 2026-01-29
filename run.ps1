# Ryo2D Development Run Script (Windows PowerShell)
# For Unix/Linux/macOS, use run.sh instead
#
# Usage:
#   .\run              - Interactive example selection
#   .\run <example>    - Run specific example (e.g., .\run breakout)
#   .\run check        - Fast compilation check
#   .\run lint         - Linting with clippy
#   .\run test         - Run tests
#   .\run watch        - Watch mode with hot-reload (requires cargo-watch)

param(
    [Parameter(Position = 0)]
    [string]$Command = ""
)

# Add cargo to PATH if not already present
if ($IsWindows -or $env:OS -eq "Windows_NT") {
    $cargoPath = "$env:USERPROFILE\.cargo\bin"
}
else {
    $cargoPath = "$HOME/.cargo/bin"
}

if (-not ($env:PATH -split [IO.Path]::PathSeparator -contains $cargoPath)) {
    $env:PATH = "$cargoPath$([IO.Path]::PathSeparator)$env:PATH"
}

# Function to discover examples from ryo.toml files
function Get-Examples {
    $examples = @()
    $examplesDir = Join-Path $PSScriptRoot "examples"
    
    if (Test-Path $examplesDir) {
        Get-ChildItem -Path $examplesDir -Directory | ForEach-Object {
            $ryoToml = Join-Path $_.FullName "ryo.toml"
            $mainRs = Join-Path $_.FullName "main.rs"
            
            # Check if it's a valid example (has main.rs)
            if (Test-Path $mainRs) {
                $name = $_.Name
                $displayName = $name
                
                # Try to read project name from ryo.toml
                if (Test-Path $ryoToml) {
                    $content = Get-Content $ryoToml -Raw
                    if ($content -match 'name\s*=\s*"([^"]+)"') {
                        $displayName = $matches[1]
                    }
                }
                
                $examples += [PSCustomObject]@{
                    Name = $name
                    DisplayName = $displayName
                    Path = $_.FullName
                }
            }
        }
    }
    
    # Force array return to prevent unwrapping
    return ,$examples
}

# Function to run an example
function Run-Example {
    param([string]$ExampleName)
    
    Write-Host ""
    Write-Host "Running example: $ExampleName" -ForegroundColor Cyan
    Write-Host ""
    cargo run --example $ExampleName
}

# Function to show interactive example selection
function Show-ExampleMenu {
    $examples = Get-Examples
    
    if ($examples.Count -eq 0) {
        Write-Host "No examples found in examples/ directory." -ForegroundColor Red
        exit 1
    }
    
    # Auto-select if only one example
    if ($examples.Count -eq 1) {
        Write-Host "Auto-selecting only example: $($examples[0].DisplayName)" -ForegroundColor Green
        Run-Example $examples[0].Name
        return
    }
    
    Write-Host ""
    Write-Host "=== Ryo2D Examples ===" -ForegroundColor Cyan
    Write-Host ""
    
    for ($i = 0; $i -lt $examples.Count; $i++) {
        $num = $i + 1
        Write-Host "  [$num] $($examples[$i].DisplayName)" -ForegroundColor White
    }
    
    Write-Host ""
    Write-Host "  [q] Quit" -ForegroundColor DarkGray
    Write-Host ""
    
    $selection = Read-Host "Select an example"
    
    if ($selection -eq "q" -or $selection -eq "Q") {
        Write-Host "Exiting." -ForegroundColor DarkGray
        exit 0
    }
    
    $index = [int]$selection - 1
    if ($index -ge 0 -and $index -lt $examples.Count) {
        Run-Example $examples[$index].Name
    } else {
        Write-Host "Invalid selection." -ForegroundColor Red
        exit 1
    }
}

# Execute the appropriate command
switch ($Command.ToLower()) {
    "" {
        Show-ExampleMenu
    }
    "check" {
        cargo check
    }
    "lint" {
        cargo clippy
    }
    "test" {
        cargo test
    }
    "watch" {
        # Watch mode - runs first example by default
        $examples = Get-Examples
        if ($examples.Count -gt 0) {
            $exampleName = $examples[0].Name
            Write-Host "Watching example: $exampleName (Ctrl+C to stop)" -ForegroundColor Cyan
            cargo watch -c -x "run --example $exampleName"
        } else {
            Write-Host "No examples found." -ForegroundColor Red
            exit 1
        }
    }
    "help" {
        Write-Host ""
        Write-Host "Ryo2D Development Script" -ForegroundColor Cyan
        Write-Host ""
        Write-Host "Usage:"
        Write-Host "  ./run              - Interactive example selection"
        Write-Host "  ./run <example>    - Run specific example (e.g., ./run breakout)"
        Write-Host "  ./run check        - Fast compilation check"
        Write-Host "  ./run lint         - Linting with clippy"
        Write-Host "  ./run test         - Run tests"
        Write-Host "  ./run watch        - Watch mode with hot-reload"
        Write-Host "  ./run help         - Show this help"
        Write-Host ""
    }
    default {
        # Try to run as an example name
        $examples = Get-Examples
        $match = $examples | Where-Object { $_.Name -eq $Command.ToLower() }
        
        if ($match) {
            Run-Example $match.Name
        } else {
            Write-Host "Unknown command or example: $Command" -ForegroundColor Red
            Write-Host ""
            Write-Host "Available examples:" -ForegroundColor Yellow
            $examples | ForEach-Object { Write-Host "  - $($_.Name)" }
            Write-Host ""
            Write-Host "Run './run help' for more options."
            exit 1
        }
    }
}

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
        # Search in both demos/ and games/ subdirectories
        $categories = @("demos", "games")
        foreach ($category in $categories) {
            $categoryPath = Join-Path $examplesDir $category
            if (Test-Path $categoryPath) {
                Get-ChildItem -Path $categoryPath -Directory | ForEach-Object {
                    # Check for config file (case-insensitive)
                    $ryoToml = Get-ChildItem -Path $_.FullName -Filter "*.toml" | 
                    Where-Object { $_.Name -ieq "Ryo.toml" } | 
                    Select-Object -First 1 -ExpandProperty FullName
                    
                    $mainRs = Join-Path $_.FullName "main.rs"
                    
                    # Check if it's a valid example (has main.rs)
                    if (Test-Path $mainRs) {
                        $name = $_.Name
                        $displayName = $name
                        
                        # Try to read project name from Ryo.toml
                        if (Test-Path $ryoToml) {
                            $content = Get-Content $ryoToml -Raw
                            if ($content -match 'name\s*=\s*"([^"]+)"') {
                                $displayName = $matches[1]
                            }
                        }
                        
                        $examples += [PSCustomObject]@{
                            Name        = $name
                            Category    = $category
                            DisplayName = $displayName
                            Path        = $_.FullName
                        }
                    }
                }
            }
        }
    }
    
    # Force array return to prevent unwrapping
    return , $examples
}

# Function to run an example (from its own directory as a standalone project)
function Run-Example {
    param(
        [string]$ExampleName,
        [string]$ExamplePath
    )
    
    Write-Host ""
    Write-Host "Running example: $ExampleName" -ForegroundColor Cyan
    Write-Host "  Path: $ExamplePath" -ForegroundColor DarkGray
    Write-Host ""
    
    # Save current directory and move to example directory
    $originalDir = Get-Location
    Set-Location $ExamplePath
    
    try {
        # Build the game
        Write-Host "Building..." -ForegroundColor Yellow
        & cargo run --manifest-path "$PSScriptRoot/Cargo.toml" --bin ryo2d -- build
        
        if ($LASTEXITCODE -eq 0) {
            # Run the built game
            Write-Host "Running..." -ForegroundColor Yellow
            & cargo run --manifest-path "$PSScriptRoot/Cargo.toml" --bin ryo2d -- run
        } else {
            Write-Host "Build failed!" -ForegroundColor Red
        }
    }
    finally {
        # Always restore original directory
        Set-Location $originalDir
    }
}

# Function to show category selection menu
function Show-CategoryMenu {
    $examples = Get-Examples
    
    if ($examples.Count -eq 0) {
        Write-Host "No examples found in examples/ directory." -ForegroundColor Red
        exit 1
    }
    
    # Group examples by category
    $demos = $examples | Where-Object { $_.Category -eq "demos" }
    $games = $examples | Where-Object { $_.Category -eq "games" }
    
    # Check if we have both categories
    $hasGames = $games.Count -gt 0
    $hasDemos = $demos.Count -gt 0
    
    # If only one category exists, skip to that category
    if ($hasGames -and -not $hasDemos) {
        Show-ExampleList $games "Games"
        return
    }
    if ($hasDemos -and -not $hasGames) {
        Show-ExampleList $demos "Demos"
        return
    }
    
    # Show category selection
    Write-Host ""
    Write-Host "=== Ryo2D Examples ===" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "  [1] Games" -ForegroundColor White
    Write-Host "  [2] Demos" -ForegroundColor White
    Write-Host ""
    Write-Host "  [q] Quit" -ForegroundColor DarkGray
    Write-Host ""
    
    Write-Host "Select a category: " -NoNewline
    $key = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")
    $selection = $key.Character
    Write-Host $selection
    Write-Host ""
    
    switch ($selection) {
        "1" { Show-ExampleList $games "Games" }
        "2" { Show-ExampleList $demos "Demos" }
        default {
            Write-Host "Exiting." -ForegroundColor DarkGray
            exit 0
        }
    }
}

# Function to show examples within a category
function Show-ExampleList {
    param(
        [array]$CategoryExamples,
        [string]$CategoryName
    )
    
    if ($CategoryExamples.Count -eq 0) {
        Write-Host "No examples found in $CategoryName." -ForegroundColor Red
        exit 1
    }
    
    # Auto-select if only one example
    if ($CategoryExamples.Count -eq 1) {
        Write-Host "Auto-selecting only example: $($CategoryExamples[0].DisplayName)" -ForegroundColor Green
        Run-Example $CategoryExamples[0].Name $CategoryExamples[0].Path
        return
    }
    
    Write-Host ""
    Write-Host "=== $CategoryName ===" -ForegroundColor Cyan
    Write-Host ""
    
    for ($i = 0; $i -lt $CategoryExamples.Count; $i++) {
        $num = $i + 1
        Write-Host "  [$num] $($CategoryExamples[$i].DisplayName)" -ForegroundColor White
    }
    
    Write-Host ""
    Write-Host "  [b] Back" -ForegroundColor DarkGray
    Write-Host "  [q] Quit" -ForegroundColor DarkGray
    Write-Host ""
    
    Write-Host "Select an example: " -NoNewline
    $key = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")
    $selection = $key.Character.ToString()
    Write-Host $selection
    Write-Host ""
    
    if ($selection -eq "b" -or $selection -eq "B") {
        Show-CategoryMenu
        return
    }
    
    # Try to parse as number
    if ($selection -match '^[0-9]$') {
        $selectedIndex = [int]$selection - 1
        if ($selectedIndex -ge 0 -and $selectedIndex -lt $CategoryExamples.Count) {
            Run-Example $CategoryExamples[$selectedIndex].Name $CategoryExamples[$selectedIndex].Path
            return
        }
    }
    
    # Any other input (including 'q') quits
    Write-Host "Exiting." -ForegroundColor DarkGray
    exit 0
}

# Execute the appropriate command
switch ($Command.ToLower()) {
    "" {
        Show-CategoryMenu
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
            $example = $examples[0]
            Write-Host "Watching example: $($example.Name) (Ctrl+C to stop)" -ForegroundColor Cyan
            Write-Host "  Path: $($example.Path)" -ForegroundColor DarkGray
            $originalDir = Get-Location
            Set-Location $example.Path
            try {
                cargo watch -c -x run
            }
            finally {
                Set-Location $originalDir
            }
        }
        else {
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
            Run-Example $match.Name $match.Path
        }
        else {
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

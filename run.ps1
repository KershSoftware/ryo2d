# Ryo2D Development Run Script (Windows PowerShell)
# For Unix/Linux/macOS, use run.sh instead
#
# Usage:
#   .\run              - Interactive example selection
#   .\run <example>    - Run specific example (e.g., .\run breakout)
#   .\run <example> --dev  - Run in dev mode with hot-reload
#   .\run --dev <example>  - Same as above
#   .\run check        - Fast compilation check (engine only)
#   .\run check-all    - Check + lint all examples in parallel
#   .\run lint         - Linting with clippy (engine only)
#   .\run test         - Run unit + integration tests
#   .\run test:unit    - Run unit tests only
#   .\run test:integration - Run integration tests only
#   .\run test:e2e     - Run e2e tests (requires GPU)
#   .\run coverage     - Unit test coverage (default)
#   .\run coverage:unit - Unit test coverage
#   .\run coverage:integration - Integration test coverage
#   .\run coverage:all - Combined coverage (unit + integration)

param(
    [Parameter(Position = 0)]
    [string]$Command = "",
    [Parameter(Position = 1)]
    [string]$Arg2 = "",
    [switch]$dev
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

# Handle --dev flag in either position
$DevMode = $dev
$ExampleName = $Command

# Check if first arg is --dev
if ($Command -eq "--dev" -or $Command -eq "-d") {
    $DevMode = $true
    $ExampleName = $Arg2
}
# Check if second arg is --dev
elseif ($Arg2 -eq "--dev" -or $Arg2 -eq "-d") {
    $DevMode = $true
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
                    $srcMainRs = Join-Path (Join-Path $_.FullName "src") "main.rs"

                    # Check if it's a valid example (has main.rs or src/main.rs)
                    if ((Test-Path $mainRs) -or (Test-Path $srcMainRs)) {
                        $name = $_.Name
                        $displayName = $name

                        # Try to read project name from Ryo.toml
                        if ($ryoToml -and (Test-Path $ryoToml)) {
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

# Function to show run mode selection menu
function Show-RunModeMenu {
    Write-Host ""
    Write-Host "  [1] Build + Run (release)" -ForegroundColor White
    Write-Host "  [2] Dev mode (hot-reload)" -ForegroundColor White
    Write-Host ""
    Write-Host "Select mode [1]: " -NoNewline
    $key = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")
    $selection = $key.Character
    Write-Host $selection

    return ($selection -eq "2")
}

# Function to run an example (from its own directory as a standalone project)
function Run-Example {
    param(
        [string]$ExampleName,
        [string]$ExamplePath,
        [bool]$UseDevMode = $false,
        [bool]$AskMode = $false
    )

    # If AskMode is true, prompt user for mode selection
    if ($AskMode) {
        $UseDevMode = Show-RunModeMenu
    }

    Write-Host ""
    Write-Host "Running example: $ExampleName" -ForegroundColor Cyan
    Write-Host "  Path: $ExamplePath" -ForegroundColor DarkGray

    # Save current directory and move to example directory
    $originalDir = Get-Location
    Set-Location $ExamplePath

    try {
        if ($UseDevMode) {
            Write-Host "  Mode: Development (hot-reload enabled)" -ForegroundColor Magenta
            Write-Host ""
            & cargo run --manifest-path "$PSScriptRoot/Cargo.toml" --bin ryo2d -- dev
        } else {
            Write-Host "  Mode: Build + Run (release)" -ForegroundColor Green
            Write-Host ""
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
        Run-Example $CategoryExamples[0].Name $CategoryExamples[0].Path -AskMode $true
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
            Run-Example $CategoryExamples[$selectedIndex].Name $CategoryExamples[$selectedIndex].Path -AskMode $true
            return
        }
    }

    # Any other input (including 'q') quits
    Write-Host "Exiting." -ForegroundColor DarkGray
    exit 0
}

# Execute the appropriate command
switch ($ExampleName.ToLower()) {
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
        # Default: run unit tests + integration tests
        Write-Host "Running unit + integration tests..." -ForegroundColor Cyan
        cargo test --lib --test integration
    }
    "test:unit" {
        Write-Host "Running unit tests only..." -ForegroundColor Cyan
        cargo test --lib
    }
    "test:integration" {
        Write-Host "Running integration tests only..." -ForegroundColor Cyan
        cargo test --test integration
    }
    "test:e2e" {
        Write-Host "Running e2e tests (includes ignored tests requiring GPU)..." -ForegroundColor Cyan
        cargo test --test e2e -- --include-ignored
    }
    "test:all" {
        Write-Host "Running ALL tests (unit + integration + e2e)..." -ForegroundColor Cyan
        cargo test --lib --test integration --test e2e -- --include-ignored
    }
    { $_ -in "coverage", "coverage:unit" } {
        Write-Host "Generating unit test coverage report..." -ForegroundColor Cyan
        Write-Host ""

        # Check if cargo-llvm-cov is installed
        $cov = Get-Command cargo-llvm-cov -ErrorAction SilentlyContinue
        if (-not $cov) {
            Write-Host "cargo-llvm-cov is not installed." -ForegroundColor Yellow
            Write-Host "Installing cargo-llvm-cov..." -ForegroundColor Cyan
            cargo install cargo-llvm-cov
            Write-Host ""
        }

        cargo llvm-cov --lib --ignore-filename-regex="cli" --html --output-dir target/llvm-cov/unit --open

        if ($LASTEXITCODE -eq 0) {
            Write-Host ""
            Write-Host "Unit coverage report generated successfully!" -ForegroundColor Green
            Write-Host "Report: target/llvm-cov/unit/index.html" -ForegroundColor DarkGray
        }
    }
    "coverage:integration" {
        Write-Host "Generating integration test coverage report..." -ForegroundColor Cyan
        Write-Host ""

        $cov = Get-Command cargo-llvm-cov -ErrorAction SilentlyContinue
        if (-not $cov) {
            Write-Host "cargo-llvm-cov is not installed." -ForegroundColor Yellow
            Write-Host "Installing cargo-llvm-cov..." -ForegroundColor Cyan
            cargo install cargo-llvm-cov
            Write-Host ""
        }

        cargo llvm-cov --test integration --ignore-filename-regex="cli" --html --output-dir target/llvm-cov/integration --open

        if ($LASTEXITCODE -eq 0) {
            Write-Host ""
            Write-Host "Integration coverage report generated successfully!" -ForegroundColor Green
            Write-Host "Report: target/llvm-cov/integration/index.html" -ForegroundColor DarkGray
        }
    }
    "coverage:all" {
        Write-Host "Generating combined coverage report (unit + integration)..." -ForegroundColor Cyan
        Write-Host ""

        $cov = Get-Command cargo-llvm-cov -ErrorAction SilentlyContinue
        if (-not $cov) {
            Write-Host "cargo-llvm-cov is not installed." -ForegroundColor Yellow
            Write-Host "Installing cargo-llvm-cov..." -ForegroundColor Cyan
            cargo install cargo-llvm-cov
            Write-Host ""
        }

        cargo llvm-cov --lib --test integration --ignore-filename-regex="cli" --html --output-dir target/llvm-cov/all --open

        if ($LASTEXITCODE -eq 0) {
            Write-Host ""
            Write-Host "Combined coverage report generated successfully!" -ForegroundColor Green
            Write-Host "Report: target/llvm-cov/all/index.html" -ForegroundColor DarkGray
        }
    }
    "check-all" {
        Write-Host ""
        Write-Host "=== Checking All Examples ===" -ForegroundColor Cyan
        Write-Host ""

        $examples = Get-Examples
        if ($examples.Count -eq 0) {
            Write-Host "No examples found." -ForegroundColor Red
            exit 1
        }

        # Also check the engine itself
        $checkTargets = @(
            [PSCustomObject]@{
                Name = "engine"
                DisplayName = "Engine (workspace)"
                Path = $PSScriptRoot
            }
        )
        $checkTargets += $examples

        Write-Host "Found $($examples.Count) examples + engine workspace" -ForegroundColor DarkGray
        Write-Host "Running cargo check and clippy in parallel..." -ForegroundColor DarkGray
        Write-Host ""

        # Create temp directory for output files
        $tempDir = Join-Path $env:TEMP "ryo2d-check-all-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
        New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

        # Start parallel jobs for each target
        $jobs = @()
        foreach ($target in $checkTargets) {
            $job = Start-Job -ScriptBlock {
                param($targetPath, $targetName, $tempDir, $cargoPath)

                # Ensure cargo is in PATH
                if (-not ($env:PATH -split [IO.Path]::PathSeparator -contains $cargoPath)) {
                    $env:PATH = "$cargoPath$([IO.Path]::PathSeparator)$env:PATH"
                }

                $result = @{
                    Name = $targetName
                    CheckPassed = $false
                    LintPassed = $false
                    CheckOutput = ""
                    LintOutput = ""
                }

                # Run cargo check
                $checkOutput = & cargo check --manifest-path "$targetPath/Cargo.toml" 2>&1 | Out-String
                $result.CheckPassed = ($LASTEXITCODE -eq 0)
                $result.CheckOutput = $checkOutput

                # Run cargo clippy
                $lintOutput = & cargo clippy --manifest-path "$targetPath/Cargo.toml" -- -D warnings 2>&1 | Out-String
                $result.LintPassed = ($LASTEXITCODE -eq 0)
                $result.LintOutput = $lintOutput

                return $result
            } -ArgumentList $target.Path, $target.Name, $tempDir, $cargoPath

            $jobs += @{
                Job = $job
                Target = $target
            }

            Write-Host "  Started: $($target.DisplayName)" -ForegroundColor DarkGray
        }

        Write-Host ""
        Write-Host "Waiting for all jobs to complete..." -ForegroundColor DarkGray

        # Collect results
        $results = @()
        $failedChecks = @()
        $failedLints = @()

        foreach ($jobInfo in $jobs) {
            $result = Receive-Job -Job $jobInfo.Job -Wait
            Remove-Job -Job $jobInfo.Job

            $target = $jobInfo.Target
            $checkStatus = if ($result.CheckPassed) { "+" } else { "X" }
            $lintStatus = if ($result.LintPassed) { "+" } else { "X" }

            $results += @{
                Name = $target.Name
                DisplayName = $target.DisplayName
                CheckPassed = $result.CheckPassed
                LintPassed = $result.LintPassed
                CheckOutput = $result.CheckOutput
                LintOutput = $result.LintOutput
            }

            if (-not $result.CheckPassed) {
                $failedChecks += @{
                    Name = $target.DisplayName
                    Output = $result.CheckOutput
                }
            }
            if (-not $result.LintPassed) {
                $failedLints += @{
                    Name = $target.DisplayName
                    Output = $result.LintOutput
                }
            }
        }

        # Clean up temp directory
        Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue

        # Display summary
        Write-Host ""
        Write-Host "=== Summary ===" -ForegroundColor Cyan
        Write-Host ""
        Write-Host ("{0,-30} {1,-10} {2,-10}" -f "Project", "Check", "Clippy") -ForegroundColor White
        Write-Host ("{0,-30} {1,-10} {2,-10}" -f "-------", "-----", "------") -ForegroundColor DarkGray

        foreach ($r in $results) {
            $checkColor = if ($r.CheckPassed) { "Green" } else { "Red" }
            $lintColor = if ($r.LintPassed) { "Green" } else { "Red" }
            $checkSymbol = if ($r.CheckPassed) { "PASS" } else { "FAIL" }
            $lintSymbol = if ($r.LintPassed) { "PASS" } else { "FAIL" }

            Write-Host ("{0,-30} " -f $r.DisplayName) -NoNewline
            Write-Host ("{0,-10} " -f $checkSymbol) -ForegroundColor $checkColor -NoNewline
            Write-Host ("{0,-10}" -f $lintSymbol) -ForegroundColor $lintColor
        }

        # Show errors if any
        if ($failedChecks.Count -gt 0 -or $failedLints.Count -gt 0) {
            Write-Host ""
            Write-Host "=== Errors ===" -ForegroundColor Red

            foreach ($fail in $failedChecks) {
                Write-Host ""
                Write-Host "[$($fail.Name)] Compilation errors:" -ForegroundColor Red
                Write-Host $fail.Output -ForegroundColor DarkGray
            }

            foreach ($fail in $failedLints) {
                Write-Host ""
                Write-Host "[$($fail.Name)] Clippy warnings/errors:" -ForegroundColor Yellow
                Write-Host $fail.Output -ForegroundColor DarkGray
            }

            Write-Host ""
            $totalFailed = ($failedChecks.Count + $failedLints.Count)
            Write-Host "Check completed with $totalFailed failure(s)." -ForegroundColor Red
            exit 1
        }
        else {
            Write-Host ""
            Write-Host "All checks passed!" -ForegroundColor Green
        }
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
        Write-Host "  ./run                    - Interactive example selection"
        Write-Host "  ./run <example>          - Run specific example (shows mode menu)"
        Write-Host "  ./run <example> --dev    - Run in dev mode with hot-reload"
        Write-Host "  ./run --dev <example>    - Same as above"
        Write-Host "  ./run check              - Fast compilation check (engine only)"
        Write-Host "  ./run check-all          - Check + lint all examples in parallel"
        Write-Host "  ./run lint               - Linting with clippy (engine only)"
        Write-Host "  ./run test               - Run unit + integration tests (default)"
        Write-Host "  ./run test:unit          - Run unit tests only"
        Write-Host "  ./run test:integration   - Run integration tests only"
        Write-Host "  ./run test:e2e           - Run e2e tests (requires GPU)"
        Write-Host "  ./run test:all           - Run ALL tests"
        Write-Host "  ./run coverage           - Unit test coverage (default)"
        Write-Host "  ./run coverage:unit      - Unit test coverage"
        Write-Host "  ./run coverage:integration - Integration test coverage"
        Write-Host "  ./run coverage:all       - Combined coverage (unit + integration)"
        Write-Host "  ./run watch              - Watch mode with hot-reload"
        Write-Host "  ./run help               - Show this help"
        Write-Host ""
    }
    default {
        # Try to run as an example name
        $examples = Get-Examples
        $match = $examples | Where-Object { $_.Name -eq $ExampleName.ToLower() }

        if ($match) {
            # If --dev flag was provided, use dev mode directly; otherwise show mode menu
            if ($DevMode) {
                Run-Example $match.Name $match.Path -UseDevMode $true
            } else {
                Run-Example $match.Name $match.Path -AskMode $true
            }
        }
        else {
            Write-Host "Unknown command or example: $ExampleName" -ForegroundColor Red
            Write-Host ""
            Write-Host "Available examples:" -ForegroundColor Yellow
            $examples | ForEach-Object { Write-Host "  - $($_.Name)" }
            Write-Host ""
            Write-Host "Run './run help' for more options."
            exit 1
        }
    }
}

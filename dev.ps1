# Ryo2D Development Script (Windows PowerShell)
# For Unix/Linux/macOS, use dev.sh instead
#
# Usage:
#   .\dev              - Interactive example selection
#   .\dev <example>    - Run specific example (e.g., .\dev breakout)
#   .\dev <example> -d|--dev       - Run in dev mode with hot-reload
#   .\dev <example> -p|--profiler  - Run in dev mode with profiler
#   .\dev <example> -r|--release   - Build and run release (no prompts)
#   .\dev -d|--dev <example>       - Same as above (flag before example)
#   .\dev -p|--profiler <example>  - Same as above (flag before example)
#   .\dev -r|--release <example>   - Same as above (flag before example)
#   .\dev demos:run-all    - Launch all demos in dev mode (parallel terminals)
#   .\dev demos:kill-all   - Kill all running demos
#   .\dev demos:tour [demo...] - Capture every page of every demo headlessly (target/demo-tour)
#   .\dev check        - Compilation check (engine + all examples) and line endings
#   .\dev lint         - Linting with clippy (engine + all examples)
#   .\dev test         - Run unit + integration tests
#   .\dev test:unit    - Run unit tests only
#   .\dev test:integration - Run integration tests only
#   .\dev test:e2e     - Run e2e tests (requires GPU)
#   .\dev coverage     - Unit test coverage (default)
#   .\dev coverage:unit - Unit test coverage
#   .\dev coverage:integration - Integration test coverage
#   .\dev coverage:all - Combined coverage (unit + integration)

param(
    [Parameter(Position = 0)]
    [string]$Command = "",
    [Parameter(Position = 1)]
    [string]$Arg2 = "",
    [switch]$dev,
    [switch]$release,
    [switch]$p
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

# Handle --dev, --release, and -p flags in either position
$DevMode = $dev
$ReleaseMode = $release
$ProfilerMode = $p
$ExampleName = $Command

# Check if first arg is --dev, --release, or -p/--profiler
if ($Command -eq "--dev" -or $Command -eq "-d") {
    $DevMode = $true
    $ExampleName = $Arg2
}
elseif ($Command -eq "--release" -or $Command -eq "-r") {
    $ReleaseMode = $true
    $ExampleName = $Arg2
}
elseif ($Command -eq "-p" -or $Command -eq "--profiler") {
    $ProfilerMode = $true
    $DevMode = $true  # Profiler implies dev mode
    $ExampleName = $Arg2
}
# Check if second arg is --dev, --release, or -p/--profiler
elseif ($Arg2 -eq "--dev" -or $Arg2 -eq "-d") {
    $DevMode = $true
}
elseif ($Arg2 -eq "--release" -or $Arg2 -eq "-r") {
    $ReleaseMode = $true
}
elseif ($Arg2 -eq "-p" -or $Arg2 -eq "--profiler") {
    $ProfilerMode = $true
    $DevMode = $true  # Profiler implies dev mode
}

# Fail if a file that was CRLF at the last push lost CRLF lines, or a file
# that was LF picked up some. Checks committed and uncommitted changes.
function Get-LineEndingCounts([byte[]]$Bytes) {
    # Latin-1 maps bytes 1:1, so counting characters counts bytes
    $text = [System.Text.Encoding]::GetEncoding(28591).GetString($Bytes)
    $crlf = [regex]::Matches($text, "`r`n").Count
    $lf = [regex]::Matches($text, "`n").Count - $crlf
    return @($crlf, $lf)
}

function Get-GitBlobBytes([string]$Spec) {
    # Read through a raw stream; PowerShell's pipeline would strip the CRs
    $psi = [System.Diagnostics.ProcessStartInfo]::new("git", "show `"$Spec`"")
    $psi.RedirectStandardOutput = $true
    $psi.UseShellExecute = $false
    $psi.WorkingDirectory = (Get-Location).Path
    $process = [System.Diagnostics.Process]::Start($psi)
    $buffer = [System.IO.MemoryStream]::new()
    $process.StandardOutput.BaseStream.CopyTo($buffer)
    $process.WaitForExit()
    # The comma keeps PowerShell from unrolling the array byte by byte
    return , $buffer.ToArray()
}

function Test-LineEndings {
    $root = git rev-parse --show-toplevel
    $base = git rev-parse --verify -q "@{upstream}" 2>$null
    if (-not $base) { $base = "HEAD" }
    $bad = $false
    foreach ($row in (git diff --numstat --diff-filter=M $base)) {
        $added, $null, $file = $row -split "`t", 3
        $path = Join-Path $root $file
        if ($added -eq "-" -or -not (Test-Path -LiteralPath $path -PathType Leaf)) { continue }
        $oldCrlf, $oldLf = Get-LineEndingCounts (Get-GitBlobBytes "${base}:$file")
        $newCrlf, $newLf = Get-LineEndingCounts ([System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $path).Path))
        if ($oldCrlf -gt 0 -and $oldLf -eq 0 -and $newLf -gt 0) {
            Write-Host "  ${file}: was CRLF, now has LF lines" -ForegroundColor Yellow
            $bad = $true
        }
        elseif ($oldCrlf -eq 0 -and $newCrlf -gt 0 -and $newLf -gt 0) {
            Write-Host "  ${file}: was LF, now mixes in CRLF lines" -ForegroundColor Yellow
            $bad = $true
        }
    }
    if ($bad) { Write-Host "Line endings changed; keep each file's existing line endings." }
    return -not $bad
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
# Returns: 0 = release, 1 = dev, 2 = dev with profiler
function Show-RunModeMenu {
    Write-Host ""
    Write-Host "  [1] Dev mode (hot-reload)" -ForegroundColor White
    Write-Host "  [2] Dev mode with profiler" -ForegroundColor Magenta
    Write-Host "  [3] Build + Run (release)" -ForegroundColor White
    Write-Host ""
    Write-Host "Select mode [1]: " -NoNewline
    $key = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")
    $selection = $key.Character
    Write-Host $selection

    switch ($selection) {
        "2" { return 2 }  # Dev with profiler
        "3" { return 0 }  # Release
        default { return 1 }  # Dev mode
    }
}

# Function to run an example (from its own directory as a standalone project)
# RunMode: 0 = release, 1 = dev, 2 = dev with profiler
function Run-Example {
    param(
        [string]$ExampleName,
        [string]$ExamplePath,
        [int]$RunMode = 0,
        [bool]$AskMode = $false
    )

    # If AskMode is true, prompt user for mode selection
    if ($AskMode) {
        $RunMode = Show-RunModeMenu
    }

    Write-Host ""
    Write-Host "Running example: $ExampleName" -ForegroundColor Cyan
    Write-Host "  Path: $ExamplePath" -ForegroundColor DarkGray

    # Save current directory and move to example directory
    $originalDir = Get-Location
    Set-Location $ExamplePath

    try {
        switch ($RunMode) {
            1 {
                # Dev mode
                Write-Host "  Mode: Development (hot-reload enabled)" -ForegroundColor Magenta
                Write-Host ""
                & cargo run --manifest-path "$PSScriptRoot/Cargo.toml" --bin ryo2d -- dev --skip-opt-check
            }
            2 {
                # Dev mode with profiler
                Write-Host "  Mode: Development with Profiler" -ForegroundColor Magenta
                Write-Host ""
                & cargo run --manifest-path "$PSScriptRoot/Cargo.toml" --bin ryo2d -- dev --skip-opt-check -p
            }
            default {
                # Release mode
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
        $autoSelected = $CategoryExamples[0]
        # Special case: profile-and-log always runs with profiler enabled
        if ($autoSelected.Name -eq "profile-and-log") {
            Write-Host "Auto-enabling profiler mode for profile-and-log demo" -ForegroundColor Magenta
            Run-Example $autoSelected.Name $autoSelected.Path -RunMode 2  # Dev with profiler
        } else {
            Run-Example $autoSelected.Name $autoSelected.Path -AskMode $true
        }
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

    $selection = Read-Host "Select an example"
    Write-Host ""

    if ($selection -eq "b" -or $selection -eq "B") {
        Show-CategoryMenu
        return
    }

    # Try to parse as number
    if ($selection -match '^\d+$') {
        $selectedIndex = [int]$selection - 1
        if ($selectedIndex -ge 0 -and $selectedIndex -lt $CategoryExamples.Count) {
            $selected = $CategoryExamples[$selectedIndex]
            # Special case: profile-and-log always runs with profiler enabled
            if ($selected.Name -eq "profile-and-log") {
                Write-Host "Auto-enabling profiler mode for profile-and-log demo" -ForegroundColor Magenta
                Run-Example $selected.Name $selected.Path -RunMode 2  # Dev with profiler
            } else {
                Run-Example $selected.Name $selected.Path -AskMode $true
            }
            return
        }
        Write-Host "Invalid selection: $selection (valid: 1-$($CategoryExamples.Count))" -ForegroundColor Red
        exit 1
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
    "demos:run-all" {
        # Kill any existing demos first
        $pidFile = Join-Path $PSScriptRoot ".demo-pids"
        if (Test-Path $pidFile) {
            Get-Content $pidFile | ForEach-Object {
                $procId = [int]$_
                Stop-Process -Id $procId -Force -ErrorAction SilentlyContinue
            }
            Remove-Item $pidFile -Force
        }

        # Get all demo directories
        $demosDir = Join-Path $PSScriptRoot "examples/demos"
        $demos = @()
        Get-ChildItem -Path $demosDir -Directory | ForEach-Object {
            $cargoToml = Join-Path $_.FullName "Cargo.toml"
            if (Test-Path $cargoToml) {
                $content = Get-Content $cargoToml -Raw
                if ($content -match 'name\s*=\s*"([^"]+)"') {
                    $demos += @{
                        Name = $matches[1]
                        Path = $_.FullName
                    }
                }
            }
        }

        if ($demos.Count -eq 0) {
            Write-Host "No demos found in examples/demos/" -ForegroundColor Yellow
            exit 1
        }

        Write-Host "Launching $($demos.Count) demos in dev mode..." -ForegroundColor Cyan
        $pids = @()

        foreach ($demo in $demos) {
            # Launch ryo2d dev in a new terminal window for each demo
            $ryo2dCmd = "cargo run --manifest-path `"$PSScriptRoot/Cargo.toml`" --bin ryo2d -- dev --skip-opt-check"
            $proc = Start-Process -FilePath "powershell" -ArgumentList "-NoExit", "-Command", "Set-Location '$($demo.Path)'; $ryo2dCmd" -PassThru -WindowStyle Normal
            $pids += $proc.Id
            Write-Host "  Started $($demo.Name) in dev mode (PID $($proc.Id))" -ForegroundColor Green
        }

        # Save PIDs
        $pids | Out-File -FilePath $pidFile

        Write-Host ""
        Write-Host "All demos launched in dev mode. Run './dev demos:kill-all' to close them." -ForegroundColor Cyan
    }
    "demos:kill-all" {
        $pidFile = Join-Path $PSScriptRoot ".demo-pids"
        if (-not (Test-Path $pidFile)) {
            Write-Host "No running demos found." -ForegroundColor Yellow
            exit 0
        }

        Write-Host "Killing demos..." -ForegroundColor Cyan
        $killed = 0
        Get-Content $pidFile | ForEach-Object {
            $procId = [int]$_
            $proc = Get-Process -Id $procId -ErrorAction SilentlyContinue
            if ($proc) {
                Stop-Process -Id $procId -Force
                Write-Host "  Killed PID $procId" -ForegroundColor Green
                $killed++
            }
        }
        Remove-Item $pidFile -Force

        if ($killed -gt 0) {
            Write-Host "Done. $killed demos terminated." -ForegroundColor Cyan
        } else {
            Write-Host "No demos were running." -ForegroundColor Yellow
        }
    }
    "demos:tour" {
        $tourArgs = @()
        if ($Arg2) { $tourArgs += $Arg2 }
        $tourArgs += $args
        cargo run -q -p demo-kit --bin demo-tour -- @tourArgs
        exit $LASTEXITCODE
    }
    "check" {
        cargo check --workspace
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        if (-not (Test-LineEndings)) { exit 1 }
    }
    "lint" {
        cargo clippy --workspace
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
        Write-Host "  ./dev                              - Interactive example selection"
        Write-Host "  ./dev <example>                    - Run specific example (shows mode menu)"
        Write-Host "  ./dev <example> -d|--dev           - Run in dev mode with hot-reload"
        Write-Host "  ./dev <example> -p|--profiler      - Run in dev mode with profiler"
        Write-Host "  ./dev <example> -r|--release       - Build and run release (no prompts)"
        Write-Host "  ./dev -d|--dev <example>           - Same as above (flag before example)"
        Write-Host "  ./dev -p|--profiler <example>      - Same as above (flag before example)"
        Write-Host "  ./dev -r|--release <example>       - Same as above (flag before example)"
        Write-Host "  ./dev demos:run-all      - Launch all demos in dev mode (parallel terminals)"
        Write-Host "  ./dev demos:kill-all     - Kill all running demos"
        Write-Host "  ./dev demos:tour [demo...] - Capture every page of every demo headlessly"
        Write-Host "  ./dev check              - Compilation check (engine + all examples) and line endings"
        Write-Host "  ./dev lint               - Linting with clippy (engine + all examples)"
        Write-Host "  ./dev test               - Run unit + integration tests (default)"
        Write-Host "  ./dev test:unit          - Run unit tests only"
        Write-Host "  ./dev test:integration   - Run integration tests only"
        Write-Host "  ./dev test:e2e           - Run e2e tests (requires GPU)"
        Write-Host "  ./dev test:all           - Run ALL tests"
        Write-Host "  ./dev coverage           - Unit test coverage (default)"
        Write-Host "  ./dev coverage:unit      - Unit test coverage"
        Write-Host "  ./dev coverage:integration - Integration test coverage"
        Write-Host "  ./dev coverage:all       - Combined coverage (unit + integration)"
        Write-Host "  ./dev watch              - Watch mode with hot-reload"
        Write-Host "  ./dev help               - Show this help"
        Write-Host ""
    }
    default {
        # Try to run as an example name
        $examples = Get-Examples
        $match = $examples | Where-Object { $_.Name -eq $ExampleName.ToLower() }

        if ($match) {
            # Special case: profile-and-log always runs with profiler enabled
            if ($match.Name -eq "profile-and-log") {
                Write-Host "Auto-enabling profiler mode for profile-and-log demo" -ForegroundColor Magenta
                Run-Example $match.Name $match.Path -RunMode 2  # Dev with profiler
            }
            # If flags were provided, run directly; otherwise show mode menu
            elseif ($ProfilerMode) {
                Run-Example $match.Name $match.Path -RunMode 2  # Dev with profiler
            } elseif ($DevMode) {
                Run-Example $match.Name $match.Path -RunMode 1  # Dev mode
            } elseif ($ReleaseMode) {
                Run-Example $match.Name $match.Path -RunMode 0  # Release
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
            Write-Host "Run './dev help' for more options."
            exit 1
        }
    }
}

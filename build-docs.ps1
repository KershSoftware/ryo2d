# Ferrum2D Documentation Build Script (Windows PowerShell)
# For Unix/Linux/macOS, use build-docs.sh instead
# Generates combined docs: mdbook (guides) + rustdoc (API reference)
# Usage: 
#   .\build-docs.ps1        → Build and open
#   .\build-docs.ps1 open   → Open last build (no rebuild)
#   .\build-docs.ps1 serve  → Build and serve with live reload

param(
    [Parameter(Position = 0)]
    [string]$Command = "build"
)

$ErrorActionPreference = "Stop"

# If open, just open the last build
if ($Command -eq "open") {
    if (Test-Path "docs/book/index.html") {
        $indexPath = Resolve-Path "docs/book/index.html"
        Start-Process $indexPath
        exit 0
    }
    else {
        Write-Host "No documentation found. Run without 'open' to build." -ForegroundColor Red
        exit 1
    }
}

if ($Command -eq "serve") {
    Write-Host "Starting documentation server with live reload..." -ForegroundColor Cyan
    & "$env:USERPROFILE\.cargo\bin\mdbook.exe" serve docs --open
    exit 0
}

if ($Command -ne "build") {
    Write-Host "Unknown command: $Command" -ForegroundColor Red
    Write-Host ""
    Write-Host "Usage:"
    Write-Host "  .\build-docs.ps1        → Build and open"
    Write-Host "  .\build-docs.ps1 open   → Open last build (no rebuild)"
    Write-Host "  .\build-docs.ps1 serve  → Build and serve with live reload"
    exit 1
}

Write-Host "Building Ferrum2D Documentation..." -ForegroundColor Cyan

# Step 1: Build rustdoc
Write-Host "`n[1/3] Building API documentation (rustdoc)..." -ForegroundColor Yellow
& "$env:USERPROFILE\.cargo\bin\cargo.exe" doc --no-deps --document-private-items
if ($LASTEXITCODE -ne 0) { throw "cargo doc failed" }

# Step 2: Build mdbook
Write-Host "`n[2/3] Building guide documentation (mdbook)..." -ForegroundColor Yellow
& "$env:USERPROFILE\.cargo\bin\mdbook.exe" build docs
if ($LASTEXITCODE -ne 0) { throw "mdbook build failed" }

# Step 3: Copy rustdoc into mdbook output
Write-Host "`n[3/3] Combining documentation..." -ForegroundColor Yellow
$apiDir = "docs/book/api"
if (Test-Path $apiDir) {
    Remove-Item -Recurse -Force $apiDir
}
Copy-Item -Recurse "target/doc" $apiDir

Write-Host "`nDocumentation built successfully!" -ForegroundColor Green
Write-Host "  Guide: docs/book/index.html"
Write-Host "  API:   docs/book/api/ferrum2d/index.html"

# Open in browser
$indexPath = Resolve-Path "docs/book/index.html"
Start-Process $indexPath

if ($Serve) {
    Write-Host "`nStarting local server..." -ForegroundColor Cyan
    & "$env:USERPROFILE\.cargo\bin\mdbook.exe" serve docs --open
}
else {
    # Default: open after build
    $indexPath = Resolve-Path "docs/book/index.html"
    Start-Process $indexPath
}

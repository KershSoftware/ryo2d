# Ferrum2D Documentation Build Script
# Generates combined docs: mdbook (guides) + rustdoc (API reference)

param(
    [switch]$Open,
    [switch]$Serve
)

$ErrorActionPreference = "Stop"

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

if ($Serve) {
    Write-Host "`nStarting local server..." -ForegroundColor Cyan
    & "$env:USERPROFILE\.cargo\bin\mdbook.exe" serve docs --open
} elseif ($Open) {
    $indexPath = Resolve-Path "docs/book/index.html"
    Start-Process $indexPath
}

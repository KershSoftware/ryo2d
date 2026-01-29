# Ryo2D Build Script (Windows PowerShell)
# For Unix/Linux/macOS, use build.sh instead
#
# Usage:
#   .\build         - Build in release mode
#   .\build debug   - Build in debug mode
#   .\build check   - Fast compilation check (no binary)

param(
    [string]$Mode = "release"
)

Write-Host "Ryo2D Build Script" -ForegroundColor Cyan
Write-Host "=====================" -ForegroundColor Cyan
Write-Host ""

switch ($Mode.ToLower()) {
    "release" {
        Write-Host "Building in RELEASE mode (optimized)..." -ForegroundColor Green
        cargo build --release
    }
    "debug" {
        Write-Host "Building in DEBUG mode (fast compile, slow runtime)..." -ForegroundColor Yellow
        cargo build
    }
    "check" {
        Write-Host "Running fast compilation check..." -ForegroundColor Cyan
        cargo check
    }
    default {
        Write-Host "Unknown mode: $Mode" -ForegroundColor Red
        Write-Host "Valid modes: release, debug, check" -ForegroundColor Yellow
        exit 1
    }
}

$exitCode = $LASTEXITCODE
if ($exitCode -eq 0) {
    Write-Host ""
    Write-Host "Build completed successfully!" -ForegroundColor Green
    if ($Mode.ToLower() -eq "release") {
        Write-Host "Binary location: target\release\ryo2d-demo.exe" -ForegroundColor Cyan
    } elseif ($Mode.ToLower() -eq "debug") {
        Write-Host "Binary location: target\debug\ryo2d-demo.exe" -ForegroundColor Cyan
    }
} else {
    Write-Host ""
    Write-Host "Build failed with exit code $exitCode" -ForegroundColor Red
    exit $exitCode
}

#!/usr/bin/env pwsh
# Ryo2D CLI Installation Script (Windows)
#
# This script builds the ryo2d CLI and creates a symlink for global access.
#
# Usage:
#   ./install.ps1           # Install to default location (~/.ryo2d/bin)
#   ./install.ps1 -Uninstall # Remove the symlink

param(
    [switch]$Uninstall,
    [string]$InstallDir = "$env:USERPROFILE\.ryo2d\bin"
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$BinaryName = "ryo2d.exe"
$TargetBinary = Join-Path $ScriptDir "target\release\$BinaryName"
$InstalledBinary = Join-Path $InstallDir $BinaryName

function Write-Status {
    param([string]$Action, [string]$Message)
    Write-Host ("{0,12} " -f $Action) -NoNewline -ForegroundColor Cyan
    Write-Host $Message
}

function Write-Success {
    param([string]$Message)
    Write-Host ("{0,12} " -f "Done") -NoNewline -ForegroundColor Green
    Write-Host $Message
}

function Write-Warning {
    param([string]$Message)
    Write-Host ("{0,12} " -f "Warning") -NoNewline -ForegroundColor Yellow
    Write-Host $Message
}

function Write-Error {
    param([string]$Message)
    Write-Host ("{0,12} " -f "Error") -NoNewline -ForegroundColor Red
    Write-Host $Message
}

# Uninstall mode
if ($Uninstall) {
    Write-Status "Removing" $InstalledBinary
    
    if (Test-Path $InstalledBinary) {
        Remove-Item $InstalledBinary -Force
        Write-Success "ryo2d CLI uninstalled"
    } else {
        Write-Warning "ryo2d CLI was not installed at $InstalledBinary"
    }
    exit 0
}

# Install mode
Write-Host ""
Write-Host "Ryo2D CLI Installer" -ForegroundColor Cyan
Write-Host "===================" -ForegroundColor Cyan
Write-Host ""

# Step 1: Build release binary
Write-Status "Building" "ryo2d CLI (release mode)..."

Push-Location $ScriptDir
try {
    $buildResult = cargo build --release --bin ryo2d 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Build failed"
        Write-Host $buildResult
        exit 1
    }
} finally {
    Pop-Location
}

# Verify binary exists
if (-not (Test-Path $TargetBinary)) {
    Write-Error "Binary not found at $TargetBinary"
    Write-Host "Make sure you have a [[bin]] entry for ryo2d in Cargo.toml"
    exit 1
}

# Step 2: Create install directory
if (-not (Test-Path $InstallDir)) {
    Write-Status "Creating" $InstallDir
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# Step 3: Copy binary (symlinks require admin on Windows, so we copy instead)
Write-Status "Installing" "to $InstalledBinary"
Copy-Item $TargetBinary $InstalledBinary -Force

# Step 4: Check if install dir is in PATH
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$InstallDir*") {
    Write-Warning "$InstallDir is not in your PATH"
    Write-Host ""
    Write-Host "Add it to your PATH with:" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  `$env:Path += `";$InstallDir`"" -ForegroundColor White
    Write-Host ""
    Write-Host "Or permanently (requires restart):" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', `$env:Path + ';$InstallDir', 'User')" -ForegroundColor White
    Write-Host ""
}

Write-Host ""
Write-Success "ryo2d CLI installed successfully!"
Write-Host ""
Write-Host "Run 'ryo2d --help' to get started." -ForegroundColor Gray
Write-Host ""

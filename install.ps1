#!/usr/bin/env pwsh
# Ryo2D CLI Installation Script (Windows)
#
# Installs the ryo2d CLI using `cargo install --path .`
# This installs to ~/.cargo/bin, which should already be in your PATH.
#
# Usage:
#   ./install.ps1           # Install to ~/.cargo/bin
#   ./install.ps1 -Uninstall # Uninstall the CLI

param(
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

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
    Write-Status "Uninstalling" "ryo2d CLI"
    
    Push-Location $ScriptDir
    try {
        & cargo uninstall ryo2d
        if ($LASTEXITCODE -eq 0) {
            Write-Success "ryo2d CLI uninstalled"
        }
    }
    finally {
        Pop-Location
    }
    exit 0
}

# Install mode
Write-Host ""
Write-Host "Ryo2D CLI Installer" -ForegroundColor Cyan
Write-Host "===================" -ForegroundColor Cyan
Write-Host ""

Write-Status "Installing" "ryo2d CLI to ~/.cargo/bin..."

Push-Location $ScriptDir
try {
    & cargo install --path . --bin ryo2d
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Installation failed"
        exit 1
    }
}
finally {
    Pop-Location
}

Write-Host ""
Write-Success "ryo2d CLI installed successfully!"
Write-Host ""
Write-Host "Run 'ryo2d --help' to get started." -ForegroundColor Gray
Write-Host ""

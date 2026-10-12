#!/usr/bin/env pwsh
# Ryo2D CLI Installation Script (Windows)
#
# Installs the ryo2d CLI to a dedicated directory and adds it to PATH.
# Installation directory: $env:LOCALAPPDATA\ryo2d\bin
#
# Usage:
#   ./install.ps1           # Install ryo2d
#   ./install.ps1 -Uninstall # Uninstall ryo2d

param(
  [switch]$Uninstall
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$InstallDir = Join-Path $env:LOCALAPPDATA "ryo2d\bin"
$BinaryPath = Join-Path $InstallDir "ryo2d.exe"

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

function Test-InPath {
  param([string]$Directory)
  $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
  return $currentPath -like "*$Directory*"
}

function Add-ToPath {
  param([string]$Directory)
    
  if (Test-InPath $Directory) {
    return $false  # Already in PATH
  }
    
  Write-Host ""
  Write-Host "The installation directory needs to be added to your PATH:" -ForegroundColor Yellow
  Write-Host "  $Directory" -ForegroundColor Gray
  Write-Host ""
  $response = Read-Host "Add to PATH? (Y/n)"
    
  if ($response -eq "" -or $response -match "^[Yy]") {
    Write-Status "Updating" "User PATH environment variable (modifying Windows registry)..."
        
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $newPath = "$currentPath;$Directory"
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        
    Write-Status "Updating" "Current session PATH..."
    # Update current session
    $env:Path = "$env:Path;$Directory"
        
    return $true
  }
    
  return $false
}

# Uninstall mode
if ($Uninstall) {
  Write-Host ""
  Write-Host "Ryo2D CLI Uninstaller" -ForegroundColor Cyan
  Write-Host "=====================" -ForegroundColor Cyan
  Write-Host ""
    
  $removed = $false
    
  if (Test-Path $BinaryPath) {
    Write-Status "Removing" "ryo2d.exe from $InstallDir"
    Remove-Item $BinaryPath -Force
    $removed = $true
  }
    
  # Remove install directory if empty
  if (Test-Path $InstallDir) {
    $items = Get-ChildItem $InstallDir -ErrorAction SilentlyContinue
    if ($null -eq $items -or $items.Count -eq 0) {
      Write-Status "Removing" "empty directory $InstallDir"
      Remove-Item $InstallDir -Force -Recurse
    }
  }
    
  if ($removed) {
    Write-Host ""
    Write-Success "ryo2d CLI uninstalled"
    Write-Host ""
    Write-Host "Note: PATH was not modified. You may want to manually remove:" -ForegroundColor Gray
    Write-Host "  $InstallDir" -ForegroundColor Gray
    Write-Host ""
  }
  else {
    Write-Warning "ryo2d CLI not found at $BinaryPath"
  }
    
  exit 0
}

# Install mode
Write-Host ""
Write-Host "Ryo2D CLI Installer" -ForegroundColor Cyan
Write-Host "===================" -ForegroundColor Cyan
Write-Host ""

# Check for Rust/Cargo
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  Write-Error "Cargo not found. Please install Rust from https://rustup.rs/"
  exit 1
}

Write-Status "Building" "ryo2d CLI in release mode..."
Write-Host ""

Push-Location $ScriptDir
try {
  & cargo build --release --bin ryo2d
  if ($LASTEXITCODE -ne 0) {
    Write-Error "Build failed"
    exit 1
  }
}
finally {
  Pop-Location
}

Write-Host ""
if (-not (Test-Path $InstallDir)) {
  Write-Status "Creating" "installation directory $InstallDir"
}
$null = New-Item -ItemType Directory -Path $InstallDir -Force

$sourceBinary = Join-Path $ScriptDir "target\release\ryo2d.exe"
Write-Status "Installing" "ryo2d.exe to $InstallDir"
Copy-Item $sourceBinary $BinaryPath -Force

# Install engine source for game projects to reference
$engineDir = Join-Path (Split-Path $InstallDir) "engine"
Write-Status "Installing" "ryo2d engine source to $(Split-Path $InstallDir)\engine"
if (Test-Path $engineDir) {
  Remove-Item $engineDir -Recurse -Force
}
$null = New-Item -ItemType Directory -Path $engineDir -Force

# Copy essential engine files
# `crates` holds ryo2d-derive (an engine dependency) and the optional crates
$filesToCopy = @("src", "crates", "Cargo.toml", "LICENSE", "README.md")
foreach ($item in $filesToCopy) {
  $source = Join-Path $ScriptDir $item
  if (Test-Path $source) {
    Copy-Item $source (Join-Path $engineDir $item) -Recurse -Force
  }
}

Write-Host ""
Write-Success "ryo2d CLI installed successfully!"

# Add to PATH
$addedToPath = Add-ToPath $InstallDir

Write-Host ""
if ($addedToPath) {
  Write-Host "Installation complete! You can now use 'ryo2d' from any directory." -ForegroundColor Green
  Write-Host ""
  Write-Host "The PATH has been updated for:" -ForegroundColor Gray
  Write-Host "  - Current terminal session (works immediately)" -ForegroundColor Gray
  Write-Host "  - Future terminal sessions" -ForegroundColor Gray
}
else {
  Write-Host "Installation complete!" -ForegroundColor Green
  Write-Host ""
  if (Test-InPath $InstallDir) {
    Write-Host "You can now use 'ryo2d' from any directory." -ForegroundColor Gray
  }
  else {
    Write-Host "To use 'ryo2d' from any directory, add this to your PATH:" -ForegroundColor Yellow
    Write-Host "  $InstallDir" -ForegroundColor Gray
  }
}

Write-Host ""
Write-Host "Run 'ryo2d --help' to get started." -ForegroundColor Cyan
Write-Host ""

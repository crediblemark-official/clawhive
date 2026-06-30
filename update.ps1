# Claw10 OS — Windows updater (PowerShell)
# Usage:
#   irm https://raw.githubusercontent.com/crediblemark-official/claw10/master/update.ps1 | iex
#
# This script downloads and runs the latest install.ps1, preserving your
# existing %USERPROFILE%\.claw10 configuration and data.

$ErrorActionPreference = "Stop"

$Repo = "crediblemark-official/claw10"
$InstallScript = "https://raw.githubusercontent.com/$Repo/master/install.ps1"
$InstallDir = if ($env:CLAW10_INSTALL_DIR) { $env:CLAW10_INSTALL_DIR } else { "$env:LOCALAPPDATA\Claw10\bin" }
$Binary = Join-Path $InstallDir "claw10.exe"

Write-Host "Claw10 OS Updater"
Write-Host "===================="
Write-Host ""

if (Test-Path $Binary) {
    try {
        $CurrentVersion = & $Binary --version 2>$null
        Write-Host "Current version: $CurrentVersion"
    } catch {
        Write-Host "Current version: unknown"
    }
} else {
    Write-Host "Claw10 is not currently installed."
    Write-Host "Run the installer instead:"
    Write-Host "  irm $InstallScript | iex"
    exit 1
}

Write-Host "Checking for updates..."
Write-Host ""

# Download and run the latest installer
Invoke-Expression (Invoke-RestMethod -Uri $InstallScript -UseBasicParsing)

Write-Host ""
if (Test-Path $Binary) {
    try {
        $NewVersion = & $Binary --version 2>$null
        Write-Host "Updated to: $NewVersion"
    } catch {
        Write-Host "Updated successfully."
    }
}
Write-Host "Your config and data in %USERPROFILE%\.claw10 have been preserved."

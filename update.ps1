# ClawHive OS — Windows updater (PowerShell)
# Usage:
#   irm https://raw.githubusercontent.com/crediblemark-official/clawhive/master/update.ps1 | iex
#
# This script downloads and runs the latest install.ps1, preserving your
# existing %USERPROFILE%\.clawhive configuration and data.

$ErrorActionPreference = "Stop"

$Repo = "crediblemark-official/clawhive"
$InstallScript = "https://raw.githubusercontent.com/$Repo/master/install.ps1"
$InstallDir = if ($env:CLAWHIVE_INSTALL_DIR) { $env:CLAWHIVE_INSTALL_DIR } else { "$env:LOCALAPPDATA\ClawHive\bin" }
$Binary = Join-Path $InstallDir "clawhive.exe"

Write-Host "ClawHive OS Updater"
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
    Write-Host "ClawHive is not currently installed."
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
Write-Host "Your config and data in %USERPROFILE%\.clawhive have been preserved."

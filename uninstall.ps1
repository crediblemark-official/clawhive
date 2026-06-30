# ClawHive OS — Windows uninstaller (PowerShell)
# Usage:
#   irm https://raw.githubusercontent.com/crediblemark-official/clawhive/master/uninstall.ps1 | iex

$ErrorActionPreference = "Stop"

$Binary = "clawhive.exe"
$InstallDir = if ($env:CLAWHIVE_INSTALL_DIR) { $env:CLAWHIVE_INSTALL_DIR } else { "$env:LOCALAPPDATA\ClawHive\bin" }
$ConfigDir = "$env:USERPROFILE\.clawhive"

Write-Host "ClawHive OS Uninstaller"
Write-Host "========================"
Write-Host ""
Write-Host "This will remove:"
Write-Host "  - Binary: $InstallDir\$Binary"
Write-Host "  - Config & data: $ConfigDir"
Write-Host "  - PATH entry from user environment (if added by installer)"
Write-Host ""

# Non-interactive mode: set $env:CLAWHIVE_UNINSTALL_FORCE = "1" to skip confirmation
if (-not $env:CLAWHIVE_UNINSTALL_FORCE) {
    $response = Read-Host "Are you sure you want to uninstall ClawHive? [y/N]"
    if ($response -notmatch "^[yY]") {
        Write-Host "Uninstall cancelled."
        exit 0
    }
}

# Remove binary
$BinaryPath = Join-Path $InstallDir $Binary
if (Test-Path $BinaryPath) {
    Remove-Item -Path $BinaryPath -Force
    Write-Host "Removed: $BinaryPath"
} else {
    Write-Host "Binary not found: $BinaryPath"
}

# Remove install directory if empty
if (Test-Path $InstallDir) {
    $Remaining = Get-ChildItem -Path $InstallDir -Recurse -ErrorAction SilentlyContinue
    if (-not $Remaining) {
        Remove-Item -Path $InstallDir -Force -Recurse
        Write-Host "Removed empty directory: $InstallDir"
    }
}

# Remove config and data directory
if (Test-Path $ConfigDir) {
    Remove-Item -Path $ConfigDir -Force -Recurse
    Write-Host "Removed: $ConfigDir"
} else {
    Write-Host "Config directory not found: $ConfigDir"
}

# Remove from user PATH
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -and $UserPath -like "*$InstallDir*") {
    $NewPath = ($UserPath -split ";" | Where-Object { $_ -ne $InstallDir }) -join ";"
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    Write-Host "Updated user PATH."
}

Write-Host ""
Write-Host "ClawHive has been uninstalled."
Write-Host "You may need to restart your terminal for PATH changes to take effect."

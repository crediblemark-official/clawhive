# Claw10 OS — Windows installer (PowerShell)
# Usage:
#   irm https://raw.githubusercontent.com/crediblemark-official/claw10/master/install.ps1 | iex
#
# To use your own domain, replace the raw GitHub URL above and host this script there.

$ErrorActionPreference = "Stop"

$Repo = "crediblemark-official/claw10"
$Binary = "claw10.exe"
$InstallDir = if ($env:CLAW10_INSTALL_DIR) { $env:CLAW10_INSTALL_DIR } else { "$env:LOCALAPPDATA\Claw10\bin" }
$CargoBuild = if ($env:CLAW10_CARGO_BUILD) { $env:CLAW10_CARGO_BUILD } else { "0" }

$Platform = "windows"
$Arch = if ([Environment]::Is64BitOperatingSystem) { "x86_64" } else { "i686" }

# Detect ARM64 Windows
if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64" -or $env:PROCESSOR_ARCHITEW6432 -eq "ARM64") {
    $Arch = "aarch64"
}

Write-Host "Installing Claw10 OS for $Platform-$Arch..."

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

$Downloaded = $false
if ($CargoBuild -eq "0") {
    $LatestUrl = "https://api.github.com/repos/$Repo/releases/latest"
    $AssetName = "claw10-$Platform-$Arch.zip"

    try {
        $Release = Invoke-RestMethod -Uri $LatestUrl -UseBasicParsing -TimeoutSec 15
        $Asset = $Release.assets | Where-Object { $_.name -eq $AssetName } | Select-Object -First 1

        if ($Asset) {
            $TmpDir = Join-Path $env:TEMP ([System.Guid]::NewGuid().ToString())
            New-Item -ItemType Directory -Force -Path $TmpDir | Out-Null
            $ZipPath = Join-Path $TmpDir $AssetName

            Write-Host "Downloading $AssetName..."
            Invoke-WebRequest -Uri $Asset.browser_download_url -OutFile $ZipPath -UseBasicParsing
            Expand-Archive -Path $ZipPath -DestinationPath $TmpDir -Force
            Copy-Item -Path (Join-Path $TmpDir $Binary) -Destination (Join-Path $InstallDir $Binary) -Force
            Remove-Item -Recurse -Force $TmpDir
            $Downloaded = $true
        }
    } catch {
        Write-Host "No prebuilt binary found or download failed. Falling back to cargo build..."
    }
}

if (-not $Downloaded) {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "Rust/Cargo is required but not installed. Install Rust first:"
        Write-Host '  https://rustup.rs/'
        exit 1
    }

    $TmpDir = Join-Path $env:TEMP ([System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Force -Path $TmpDir | Out-Null
    git clone --depth 1 "https://github.com/$Repo.git" "$TmpDir\claw10" 2>$null
    if (-not (Test-Path "$TmpDir\claw10")) {
        Write-Host "Failed to clone repository. Make sure git is installed."
        exit 1
    }
    cargo build --release --manifest-path "$TmpDir\claw10\Cargo.toml"
    Copy-Item -Path "$TmpDir\claw10\target\release\$Binary" -Destination (Join-Path $InstallDir $Binary) -Force
    Remove-Item -Recurse -Force $TmpDir
}

# Add to PATH
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    Write-Host "Added $InstallDir to user PATH."
}

Write-Host ""
Write-Host "Claw10 OS installed to: $InstallDir\$Binary"
Write-Host "Run 'claw10 --help' to get started."
Write-Host "Run 'claw10 setup' for initial configuration wizard."

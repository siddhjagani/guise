# guise installer (Windows).
#
#   powershell -ExecutionPolicy Bypass -File install.ps1
#
# Builds from source and installs the `guise.exe` binary to
# %LOCALAPPDATA%\guise\bin. Add that dir to your PATH to run `guise` directly.

$ErrorActionPreference = "Stop"

$RepoDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$BinName = "guise.exe"

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "cargo (the Rust toolchain) is required. Install from https://rustup.rs"
    exit 1
}

Write-Host "Building $BinName (release)..."
cargo build --release --manifest-path (Join-Path $RepoDir "Cargo.toml")

$InstallDir = Join-Path $env:LOCALAPPDATA "guise\bin"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item -Force (Join-Path $RepoDir "target\release\$BinName") (Join-Path $InstallDir $BinName)

Write-Host "Installed to $InstallDir\$BinName"
Write-Host "Add $InstallDir to your PATH to run 'guise' directly."
Write-Host "Run 'guise doctor' to verify your setup, then 'guise add <name>'."

# guise installer (Windows placeholder).
#
# NOTE: guise v1 supports macOS only. The storage/keychain layer is written
# behind a `CredentialBackend` trait so a Windows (DPAPI + %APPDATA%\Claude)
# backend can be added later, but it is not implemented yet. This script builds
# the binary so contributors can work on that backend.

$ErrorActionPreference = "Stop"

$RepoDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$BinName = "guise.exe"

Write-Host "guise v1 targets macOS. On Windows this only builds the binary; the"
Write-Host "Windows CredentialBackend is not implemented yet." -ForegroundColor Yellow

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

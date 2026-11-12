# guise installer (Windows).
#
#   irm https://raw.githubusercontent.com/siddhjagani/guise/main/install.ps1 | iex
#
# Works two ways:
#   * piped into iex (no clone)       -> downloads a prebuilt guise.exe (x64 or
#                                        ARM64) from the latest GitHub release
#   * run inside a cloned checkout    -> builds from source with cargo:
#       powershell -ExecutionPolicy Bypass -File install.ps1
#
# Installs to %LOCALAPPDATA%\guise\bin and adds that dir to your user PATH.
#
# Env overrides:
#   $env:GUISE_VERSION = 'v1.2.3'     install a specific release tag (default: latest)
#
# Runs on Windows PowerShell 5.1 and PowerShell 7+. Everything lives in one
# function that fails with `throw`, never `exit`, so an error while piped into
# iex can't close your shell. Keep this file ASCII: 5.1 reads BOM-less scripts
# in the ANSI code page.

function Install-Guise {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue' # 5.1's progress bar makes downloads crawl

    if ($env:OS -ne 'Windows_NT') { throw 'install.ps1 is for Windows. On macOS use install.sh.' }

    $repo = 'siddhjagani/guise'
    $installDir = Join-Path $env:LOCALAPPDATA 'guise\bin'
    $dest = Join-Path $installDir 'guise.exe'
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null

    # $PSScriptRoot is empty when piped into iex, so this only builds inside a checkout.
    if ($PSScriptRoot -and (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'Cargo.toml')) -and (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host 'Building guise from source (release)...'
        $manifest = Join-Path $PSScriptRoot 'Cargo.toml'
        # Continue: cargo's progress on stderr must not turn into a terminating error (ISE, redirected output).
        & { $ErrorActionPreference = 'Continue'; cargo build --release --manifest-path $manifest }
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit code $LASTEXITCODE)." }
        Copy-Item -Force -LiteralPath (Join-Path $PSScriptRoot 'target\release\guise.exe') -Destination $dest
    } else {
        $asset = 'guise-windows-x86_64.zip'
        if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64' -or $env:PROCESSOR_ARCHITEW6432 -eq 'ARM64') {
            $asset = 'guise-windows-aarch64.zip'
        }
        $ver = $env:GUISE_VERSION
        if (-not $ver) { $ver = 'latest' }
        if ($ver -eq 'latest') {
            $url = "https://github.com/$repo/releases/latest/download/$asset"
        } else {
            $url = "https://github.com/$repo/releases/download/$ver/$asset"
        }

        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        $tmp = Join-Path ([IO.Path]::GetTempPath()) ('guise-install-' + [Guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $tmp | Out-Null
        try {
            Write-Host "Downloading guise ($ver, $asset)..."
            $zip = Join-Path $tmp $asset
            try {
                Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $zip
            } catch {
                throw ("could not download $url ($($_.Exception.Message))`n" +
                    "No release yet? Build from source instead (needs Rust: https://rustup.rs):`n" +
                    "  git clone https://github.com/$repo; cd guise; powershell -ExecutionPolicy Bypass -File install.ps1")
            }
            Expand-Archive -LiteralPath $zip -DestinationPath $tmp
            Copy-Item -Force -LiteralPath (Join-Path $tmp 'guise.exe') -Destination $dest
        } finally {
            Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
    Write-Host "Installed $dest"

    # True when the ;-separated $List already holds $installDir (case-insensitive, %VARS% expanded, trailing \ ignored).
    function Test-OnPath([string]$List) {
        @($List -split ';' | Where-Object { $_ -and [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\') -eq $installDir }).Count -gt 0
    }

    # Read the raw value so %VAR% entries survive, and write it back as REG_EXPAND_SZ.
    $userPath = (Get-Item -Path 'HKCU:\Environment').GetValue('Path', '', 'DoNotExpandEnvironmentNames')
    if (Test-OnPath $userPath) {
        Write-Host "$installDir is already on your user PATH."
    } else {
        $newPath = $installDir
        if ($userPath) { $newPath = $userPath.TrimEnd(';') + ';' + $installDir }
        Set-ItemProperty -Path 'HKCU:\Environment' -Name 'Path' -Value $newPath -Type ExpandString
        # Deleting a dummy user variable makes .NET broadcast WM_SETTINGCHANGE, so new terminals see the PATH.
        [Environment]::SetEnvironmentVariable('GUISE_INSTALL_TMP', $null, 'User')
        Write-Host "Added $installDir to your user PATH. Open a new terminal if 'guise' is not found."
    }
    if (-not (Test-OnPath $env:Path)) { $env:Path = $env:Path.TrimEnd(';') + ';' + $installDir }

    Write-Host "Run 'guise doctor' to verify your setup, then 'guise add <name>'."
}

Install-Guise

# End-to-end test of guise on real Windows (run by CI on Windows runners):
# drives the built guise.exe against a stand-in Claude, in a throwaway store
# whose path contains spaces.
#
#   pwsh tests/windows/e2e.ps1 [path\to\guise.exe]
param([string]$Guise = 'target\release\guise.exe')
$ErrorActionPreference = 'Stop'

$Guise = (Resolve-Path $Guise).Path
$root = Join-Path ([IO.Path]::GetTempPath()) "guise e2e $PID"
New-Item -ItemType Directory -Force -Path $root | Out-Null
$env:XDG_CONFIG_HOME = Join-Path $root 'config dir'
$store = Join-Path $env:XDG_CONFIG_HOME 'guise'
$fake = Join-Path $root 'claude.exe'

function Check([bool]$ok, [string]$what) {
    if (-not $ok) { throw "FAIL: $what" }
    Write-Host "ok - $what"
}
function Guise { & $Guise @args; if ($LASTEXITCODE) { throw "guise $args exited $LASTEXITCODE" } }
function Running([string]$name) {
    ((& $Guise ls --json | ConvertFrom-Json) | Where-Object name -eq $name).running
}
function WaitFor([scriptblock]$cond, [string]$what) {
    $deadline = (Get-Date).AddSeconds(20)
    while (-not (& $cond)) {
        if ((Get-Date) -gt $deadline) { throw "FAIL: timed out waiting for $what" }
        Start-Sleep -Milliseconds 250
    }
    Write-Host "ok - $what"
}
# Run guise with its output captured, as a script would. Fails if guise keeps
# the caller waiting (the launched app must not inherit guise's stdout pipe).
function Captured([string[]]$argv) {
    $job = Start-Job { param($g, $a) & $g @a 2>&1 | Out-String } -ArgumentList $Guise, $argv
    if (-not (Wait-Job $job -Timeout 30)) { Stop-Job $job; throw "FAIL: 'guise $argv' with captured output hung" }
    Receive-Job $job
}

try {
    rustc -O --edition 2021 (Join-Path $PSScriptRoot 'fake-claude.rs') -o $fake
    if ($LASTEXITCODE) { throw 'building the stand-in Claude failed' }

    Guise config set app-path $fake
    $doctor = & $Guise doctor --json | ConvertFrom-Json
    Check $doctor.healthy 'doctor is healthy with Claude present'

    # add: launches this account's own instance, and captured output returns.
    $out = Captured @('add', 'work')
    Check ($out -match 'Created work') 'add reports the new account'
    WaitFor { Running 'work' } 'ls shows work running'
    $data = Join-Path $store 'accounts\1-work\data'
    Check (Test-Path (Join-Path $data 'config.json')) 'Claude got the account''s own --user-data-dir'

    # open while running: hands off to the running instance (focus), no second copy.
    Guise work
    WaitFor { Test-Path (Join-Path $data 'focused') } 'open on a running account focuses it'
    WaitFor { @(Get-Process claude -ErrorAction SilentlyContinue).Count -eq 1 } 'still exactly one instance'

    # Chat sharing (opt-in on Windows): a junction to the shared folder.
    Guise config set share-code-sessions on
    $out = Captured @('add', 'personal')
    WaitFor { Running 'personal' } 'second account runs alongside the first'
    $link = Get-Item (Join-Path $store 'accounts\2-personal\data\claude-code-sessions')
    Check ($link.LinkType -eq 'Junction') 'chats folder is a junction'
    $shared = Join-Path $store 'claude-code-sessions'
    Set-Content (Join-Path $shared 'keep.txt') 'shared chat'
    $doctor = & $Guise doctor --json | ConvertFrom-Json
    Check (($doctor.accounts | Where-Object name -eq 'personal').chats_shared) 'doctor sees the chats shared'

    # rm: quits the instance, deletes the profile, never the shared chats.
    Guise rm personal
    Check (-not (Test-Path (Join-Path $store 'accounts\2-personal'))) 'rm deletes the profile'
    Check (Test-Path (Join-Path $shared 'keep.txt')) 'rm leaves the shared chats alone'
    Check (Running 'work') 'rm leaves other accounts running'
    Guise rm work
    Check (@(Get-Process claude -ErrorAction SilentlyContinue).Count -eq 0) 'rm quits Claude'
    Check (@(& $Guise ls --json | ConvertFrom-Json).Count -eq 0) 'no accounts left'

    Write-Host 'All Windows end-to-end checks passed.'
} finally {
    Get-Process claude -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
}

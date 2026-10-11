# Builds and tests the Rust engine on Windows, and the desktop client (apps\desktop): format,
# lints, build and the whole test suite of each.
# It is not a merge-gate step (scripts/check_all.sh stays the only list of those); run it on a
# Windows machine or through scripts/vmpal_check_engine.sh from a Mac.
#
# Needs: rustup, git (the version-control tests run a real repository), the Visual Studio C++
# build tools (MSVC and a Windows SDK), `tar` and `icacls`
# (both ship with Windows), and the right to create symbolic links (Developer Mode, or an
# elevated shell), because tests check that links are never followed. On ARM64 the C code of
# `ring` also needs clang (winget install LLVM.LLVM).
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$env:PATH = "$HOME\.cargo\bin;C:\Program Files\LLVM\bin;C:\Program Files\Git\cmd;$env:PATH"
$jobs = if ($env:SOYEHT_JOBS) { $env:SOYEHT_JOBS } else { '4' }

foreach ($tool in 'cargo', 'rustup', 'git') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "check_engine_windows: '$tool' is required but not on PATH"
    }
}

# A symbolic link is how the tests prove the engine never follows one.
$probe = Join-Path ([IO.Path]::GetTempPath()) "soyeht-symlink-probe-$PID"
try {
    New-Item -ItemType SymbolicLink -Path $probe -Target $repoRoot -ErrorAction Stop | Out-Null
    (Get-Item $probe).Delete()
} catch {
    throw 'check_engine_windows: this account cannot create symbolic links; turn on Developer Mode or use an elevated shell'
}

function Invoke-Step {
    param([string]$Name, [scriptblock]$Command)
    Write-Host "check_engine_windows: $Name"
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "check_engine_windows: $Name failed with exit code $LASTEXITCODE"
    }
}

Set-Location (Join-Path $repoRoot 'engine')
Invoke-Step 'cargo fmt' { cargo fmt --all --check }
Invoke-Step 'cargo clippy' { cargo clippy --locked --workspace --all-targets -j $jobs -- -D warnings }
Invoke-Step 'cargo build' { cargo build --locked --workspace -j $jobs }
Invoke-Step 'cargo test' { cargo test --locked --workspace -j $jobs }

# The desktop client is its own workspace; its Windows code compiles only here.
Set-Location (Join-Path $repoRoot 'apps\desktop')
Invoke-Step 'desktop cargo fmt' { cargo fmt --all --check }
Invoke-Step 'desktop cargo clippy' { cargo clippy --locked --workspace --all-targets -j $jobs -- -D warnings }
Invoke-Step 'desktop cargo test' { cargo test --locked --workspace -j $jobs }
Write-Host 'check_engine_windows: passed'

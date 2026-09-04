# SPDX-FileCopyrightText: 2026 Greg Wuller
# SPDX-License-Identifier: MIT
#
# Build a double-clickable snd-review.exe and optionally install it for
# the Start Menu and CLI use.

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Test-IsWindows {
    if ($null -ne (Get-Variable -Name IsWindows -Scope Global -ErrorAction SilentlyContinue)) {
        return [bool]$IsWindows
    }
    return $env:OS -eq "Windows_NT"
}

function Show-Usage {
    @"
Usage: script/bundle-windows.ps1 [--install]

Build a Windows executable at target/release/snd-review.exe.

      --install   Copy the exe to %LOCALAPPDATA%\snd-review\snd-review.exe,
                  copy it to %USERPROFILE%\.local\bin\snd-review.exe, and
                  add a Start Menu shortcut
  -h, --help      Show this help
"@
}

if (-not (Test-IsWindows)) {
    Write-Error "script/bundle-windows.ps1 only runs on Windows"
    exit 1
}

$install = $false
foreach ($arg in $args) {
    switch ($arg) {
        "--install" { $install = $true }
        "-Install" { $install = $true }
        "-h" { Show-Usage; exit 0 }
        "--help" { Show-Usage; exit 0 }
        default {
            Write-Error "unknown argument: $arg"
            Show-Usage
            exit 1
        }
    }
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $root

$cargoToml = Get-Content -Raw (Join-Path $root "Cargo.toml")
if ($cargoToml -notmatch '(?m)^version = "([^"]+)"') {
    Write-Error "could not read version from Cargo.toml"
    exit 1
}
$version = $Matches[1]

Write-Host "Building snd-review $version (release)..."
cargo build --release
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root "target" }
$binary = Join-Path $targetDir "release\snd-review.exe"
if (-not (Test-Path -LiteralPath $binary)) {
    Write-Error "missing release binary: $binary"
    exit 1
}

Write-Host "Created $binary"

if (-not $install) {
    exit 0
}

$appDestDir = Join-Path $env:LOCALAPPDATA "snd-review"
$appDest = Join-Path $appDestDir "snd-review.exe"
$binDir = Join-Path $env:USERPROFILE ".local\bin"
$cliLink = Join-Path $binDir "snd-review.exe"

New-Item -ItemType Directory -Force -Path $appDestDir | Out-Null
Copy-Item -Force -LiteralPath $binary -Destination $appDest

New-Item -ItemType Directory -Force -Path $binDir | Out-Null
Remove-Item -Force -ErrorAction SilentlyContinue -LiteralPath $cliLink
try {
    New-Item -ItemType HardLink -Path $cliLink -Target $appDest | Out-Null
}
catch {
    Copy-Item -Force -LiteralPath $appDest -Destination $cliLink
}

$startMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
New-Item -ItemType Directory -Force -Path $startMenu | Out-Null
$shortcutPath = Join-Path $startMenu "snd-review.lnk"
$wsh = New-Object -ComObject WScript.Shell
$shortcut = $wsh.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $appDest
$shortcut.WorkingDirectory = $appDestDir
$shortcut.Description = "Scrollable, zoomable multi-channel waveform display"
$shortcut.IconLocation = "$appDest,0"
$shortcut.Save()

Write-Host "Installed $appDest"
Write-Host "Linked $cliLink -> $appDest"
Write-Host "Created Start Menu shortcut $shortcutPath"

$pathEntries = ($env:PATH -split ";") | ForEach-Object { $_.TrimEnd("\") }
$binNormalized = $binDir.TrimEnd("\")
$onPath = $pathEntries | Where-Object { $_.ToLowerInvariant() -eq $binNormalized.ToLowerInvariant() }
if (-not $onPath) {
    Write-Host @"
warning: $binDir is not on PATH.
Add the following in PowerShell, then open a new terminal:

  `$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  [Environment]::SetEnvironmentVariable("Path", "`$userPath;$binDir", "User")
"@
}

# stfu one-command installer for Windows.
#
#   irm https://raw.githubusercontent.com/joe-lloyd/stfu/main/scripts/install.ps1 | iex
#
# Downloads the latest GitHub release, clears the "downloaded from the internet" mark that
# triggers SmartScreen's unknown-publisher warning, and runs the installer silently for the
# current user. Set $env:STFU_VERSION = "v0.1.0" first to pin a release.
$ErrorActionPreference = "Stop"

$repo = "joe-lloyd/stfu"
$api = if ($env:STFU_VERSION) { "https://api.github.com/repos/$repo/releases/tags/$($env:STFU_VERSION)" }
       else { "https://api.github.com/repos/$repo/releases/latest" }

function Say($msg)  { Write-Host "==> $msg" -ForegroundColor Cyan }
function Warn($msg) { Write-Host "note: $msg" -ForegroundColor Yellow }

if ([Environment]::OSVersion.Version.Major -lt 10) { throw "stfu needs Windows 10 or newer" }

Say "Finding the latest Windows build"
$release = Invoke-RestMethod -Uri $api -Headers @{ "User-Agent" = "stfu-installer" }
$asset = $release.assets | Where-Object { $_.name -like "*-setup.exe" } | Select-Object -First 1
if (-not $asset) { $asset = $release.assets | Where-Object { $_.name -like "*.msi" } | Select-Object -First 1 }
if (-not $asset) { throw "No Windows installer found in release $($release.tag_name)" }

$tmp = Join-Path $env:TEMP $asset.name
Say "Downloading $($asset.name)"
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $tmp -UseBasicParsing

Say "Clearing the SmartScreen download mark"
Unblock-File -Path $tmp

Say "Stopping any running copy"
Get-Process -Name stfu -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

Say "Installing"
if ($tmp -like "*.msi") {
  $p = Start-Process msiexec.exe -ArgumentList "/i", "`"$tmp`"", "/qn" -Wait -PassThru
} else {
  $p = Start-Process $tmp -ArgumentList "/S" -Wait -PassThru   # NSIS silent, per-user install
}
if ($p.ExitCode -ne 0) { throw "the installer exited with code $($p.ExitCode)" }

$exe = Join-Path $env:LOCALAPPDATA "stfu\stfu.exe"
if (-not (Test-Path $exe)) { $exe = Join-Path $env:ProgramFiles "stfu\stfu.exe" }
if (-not (Test-Path $exe)) { throw "installed, but stfu.exe was not where expected" }

Say "Starting stfu"
Start-Process $exe
Start-Sleep -Seconds 3
if (-not (Get-Process -Name stfu -ErrorAction SilentlyContinue)) {
  Warn "stfu does not appear to be running. Start it from the Start menu and watch for an error."
}

Write-Host ""
Write-Host "Installed stfu $($release.tag_name)." -ForegroundColor Green
Write-Host @"

stfu runs in the system tray, not a window. Windows hides new tray icons by default, so click
the ^ arrow next to the clock to find the waveform icon, and drag it onto the taskbar to keep
it visible.

Its Settings window opens by itself. Add an API key for speech to text (console.groq.com is
free and fast), then hold Ctrl+Win anywhere and talk.

Two things worth knowing:
  - If nothing is recorded, allow desktop apps to use the microphone:
    Settings > Privacy & security > Microphone.
  - The hotkey cannot reach windows running as Administrator unless stfu runs as Administrator
    too. Everything else works normally.

"@

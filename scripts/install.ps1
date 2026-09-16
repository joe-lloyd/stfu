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

function Say($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

Say "Finding latest Windows build"
$release = Invoke-RestMethod -Uri $api -Headers @{ "User-Agent" = "stfu-installer" }
$asset = $release.assets | Where-Object { $_.name -like "*-setup.exe" } | Select-Object -First 1
if (-not $asset) { $asset = $release.assets | Where-Object { $_.name -like "*.msi" } | Select-Object -First 1 }
if (-not $asset) { throw "No Windows installer found in release $($release.tag_name)" }

$tmp = Join-Path $env:TEMP $asset.name
Say "Downloading $($asset.name)"
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $tmp -UseBasicParsing

Say "Clearing SmartScreen quarantine mark"
Unblock-File -Path $tmp

Say "Stopping any running copy"
Get-Process -Name stfu -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

Say "Installing"
if ($tmp -like "*.msi") {
  Start-Process msiexec.exe -ArgumentList "/i", "`"$tmp`"", "/qn" -Wait
} else {
  Start-Process $tmp -ArgumentList "/S" -Wait   # NSIS silent, per-user install
}

$exe = Join-Path $env:LOCALAPPDATA "stfu\stfu.exe"
if (-not (Test-Path $exe)) { $exe = Join-Path $env:ProgramFiles "stfu\stfu.exe" }
if (Test-Path $exe) {
  Say "Launching"
  Start-Process $exe
}

Write-Host ""
Write-Host "Installed stfu $($release.tag_name)."
Write-Host "Hold Ctrl+Win anywhere and talk. Settings (providers and API keys) are in the tray icon menu."
Write-Host "The keyboard hook cannot see windows running as Administrator unless stfu also runs elevated."

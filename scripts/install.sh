#!/bin/sh
# stfu one-command installer for macOS and Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/joe-lloyd/stfu/main/scripts/install.sh | sh
#
# Downloads the latest GitHub release, installs it, and removes the download quarantine flag so
# the unsigned build opens without the "cannot be opened because the developer cannot be
# verified" dialog. Set STFU_VERSION=v0.1.0 to pin a release.
set -eu

REPO="joe-lloyd/stfu"
API="https://api.github.com/repos/$REPO/releases"
if [ -n "${STFU_VERSION:-}" ]; then API="$API/tags/$STFU_VERSION"; else API="$API/latest"; fi

say()  { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mnote:\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

fetch() { # fetch URL [outfile]
  if command -v curl >/dev/null 2>&1; then
    if [ $# -gt 1 ]; then curl -fsSL "$1" -o "$2"; else curl -fsSL "$1"; fi
  elif command -v wget >/dev/null 2>&1; then
    if [ $# -gt 1 ]; then wget -qO "$2" "$1"; else wget -qO- "$1"; fi
  else
    die "need curl or wget"
  fi
}

asset_url() { # asset_url <suffix-or-substring>
  # Pull browser_download_url lines out of the release JSON without needing jq.
  fetch "$API" | tr ',' '\n' | sed -n 's/.*"browser_download_url": *"\([^"]*\)".*/\1/p' | grep -- "$1" | head -n 1
}

TMP="$(mktemp -d)"
cleanup() { [ -n "${MOUNT:-}" ] && hdiutil detach "$MOUNT" -quiet 2>/dev/null; rm -rf "$TMP"; }
trap cleanup EXIT

case "$(uname -s)" in
  Darwin)
    MAJOR="$(sw_vers -productVersion | cut -d. -f1)"
    [ "$MAJOR" -ge 12 ] 2>/dev/null || die "stfu needs macOS 12 or newer (found $(sw_vers -productVersion))"

    say "Finding the latest macOS build"
    URL="$(asset_url .dmg)"; [ -n "$URL" ] || die "no .dmg asset in the release"
    say "Downloading $(basename "$URL")"
    fetch "$URL" "$TMP/stfu.dmg"

    say "Mounting the disk image"
    MOUNT="$(hdiutil attach -nobrowse -readonly -mountrandom "$TMP" "$TMP/stfu.dmg" \
             | awk -F'\t' '/\/Volumes|\/private|\/var|\/tmp/ {print $NF}' | tail -n 1)"
    [ -n "$MOUNT" ] && [ -d "$MOUNT" ] || die "could not mount the disk image"
    APP="$(find "$MOUNT" -maxdepth 1 -name '*.app' | head -n 1)"
    [ -n "$APP" ] || die "no .app inside the disk image"

    say "Installing to /Applications"
    osascript -e 'quit app "stfu"' 2>/dev/null || true
    pkill -x stfu 2>/dev/null || true
    sleep 1
    rm -rf "/Applications/stfu.app"
    cp -R "$APP" /Applications/ || die "could not write to /Applications (is it locked down?)"

    say "Clearing the download quarantine so Gatekeeper lets it open"
    xattr -dr com.apple.quarantine /Applications/stfu.app 2>/dev/null || true

    # A broken signature is the one failure that would silently stop macOS remembering the
    # permissions you are about to grant, so check rather than assume.
    if ! codesign --verify --deep --strict /Applications/stfu.app 2>/dev/null; then
      warn "the app's code signature did not verify; permissions may not stick. Reinstalling usually fixes it."
    fi

    say "Starting stfu"
    open /Applications/stfu.app || die "the app would not start"
    sleep 3
    pgrep -x stfu >/dev/null 2>&1 || warn "stfu does not appear to be running; open it from /Applications and watch for an error."

    cat <<'MSG'

Installed. stfu lives in the menu bar (the small waveform icon), not the Dock.

Its Settings window opens by itself and lists three macOS permissions with a button each:

  Microphone         hears you
  Input Monitoring   notices the hotkey
  Accessibility      pastes the text into whatever app you are in

macOS only ever asks once per permission, so if you miss a prompt use those buttons: they open
the right page in System Settings. The panel turns green as each one lands, and stfu restarts
itself so the change takes effect.

Then add an API key for speech to text (console.groq.com is free and fast), hold the Fn key
anywhere, and talk.

MSG
    ;;
  Linux)
    say "Finding the latest Linux build"
    URL="$(asset_url .AppImage)"; [ -n "$URL" ] || die "no .AppImage asset in the release"
    DEST="${XDG_BIN_HOME:-$HOME/.local/bin}"
    mkdir -p "$DEST"
    say "Downloading $(basename "$URL") to $DEST/stfu"
    fetch "$URL" "$DEST/stfu"
    chmod +x "$DEST/stfu"
    case ":$PATH:" in *":$DEST:"*) ;; *) warn "$DEST is not on your PATH" ;; esac
    cat <<MSG

Installed $DEST/stfu. Run it with: stfu
Linux support is best-effort (X11 only for the global hotkey). Default hotkey: Ctrl+Super.
MSG
    ;;
  *)
    die "unsupported OS: $(uname -s). On Windows use install.ps1."
    ;;
esac

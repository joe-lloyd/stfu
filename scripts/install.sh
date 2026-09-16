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

say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

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
trap 'rm -rf "$TMP"' EXIT

case "$(uname -s)" in
  Darwin)
    say "Finding latest macOS build"
    URL="$(asset_url .dmg)"; [ -n "$URL" ] || die "no .dmg asset in the release"
    say "Downloading $(basename "$URL")"
    fetch "$URL" "$TMP/stfu.dmg"
    say "Mounting"
    MOUNT="$(hdiutil attach -nobrowse -readonly -mountrandom "$TMP" "$TMP/stfu.dmg" | awk -F'\t' '/\/Volumes|\/private|\/var/ {print $NF}' | tail -n 1)"
    [ -d "$MOUNT" ] || die "could not mount the disk image"
    APP="$(find "$MOUNT" -maxdepth 1 -name '*.app' | head -n 1)"
    [ -n "$APP" ] || { hdiutil detach "$MOUNT" -quiet || true; die "no .app inside the disk image"; }
    say "Installing to /Applications (quitting any running copy)"
    pkill -x stfu 2>/dev/null || true
    rm -rf "/Applications/stfu.app"
    cp -R "$APP" /Applications/
    hdiutil detach "$MOUNT" -quiet || true
    say "Removing download quarantine so Gatekeeper lets it open"
    xattr -dr com.apple.quarantine /Applications/stfu.app 2>/dev/null || true
    say "Launching"
    open /Applications/stfu.app
    cat <<'MSG'

Installed /Applications/stfu.app.

First launch asks for three permissions: Microphone, Accessibility and Input Monitoring.
Allow each; the app restarts itself once Accessibility is granted. Then hold Fn anywhere and talk.
Settings (providers and API keys) live in the menu bar icon.
MSG
    ;;
  Linux)
    say "Finding latest Linux build"
    URL="$(asset_url .AppImage)"; [ -n "$URL" ] || die "no .AppImage asset in the release"
    DEST="${XDG_BIN_HOME:-$HOME/.local/bin}"
    mkdir -p "$DEST"
    say "Downloading $(basename "$URL") to $DEST/stfu"
    fetch "$URL" "$DEST/stfu"
    chmod +x "$DEST/stfu"
    case ":$PATH:" in *":$DEST:"*) ;; *) printf 'note: %s is not on your PATH\n' "$DEST" ;; esac
    cat <<MSG

Installed $DEST/stfu. Run it with: stfu
Linux support is best-effort (X11 only for the global hotkey). Default hotkey: Ctrl+Super.
MSG
    ;;
  *)
    die "unsupported OS: $(uname -s). On Windows use install.ps1."
    ;;
esac

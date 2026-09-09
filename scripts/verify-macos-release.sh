#!/usr/bin/env bash
# Verify both the build output and the app users receive inside the DMG.
set -euo pipefail

if [[ $# != 2 || -z "${APPLE_TEAM_ID:-}" ]]; then
  echo 'Usage: APPLE_TEAM_ID=<team> bash scripts/verify-macos-release.sh <Tidy.app> <Tidy.dmg>' >&2
  exit 1
fi
app="$1"
dmg="$2"
[[ -d "$app" && -f "$dmg" ]] || { echo 'App or DMG is missing.' >&2; exit 1; }

verify_app() {
  local candidate="$1" executable metadata
  codesign --verify --deep --strict --verbose=2 "$candidate"
  for executable in "$candidate/Contents/MacOS/appflower" "$candidate/Contents/MacOS/appflower-mcp" "$candidate/Contents/MacOS/tidy-ai"; do
    lipo "$executable" -verify_arch arm64
    codesign --verify --strict --verbose=2 "$executable"
    metadata=$(codesign -d --verbose=4 "$executable" 2>&1)
    if ! grep -Fqx "TeamIdentifier=$APPLE_TEAM_ID" <<< "$metadata" \
      || ! grep -q '^Authority=Developer ID Application:' <<< "$metadata" \
      || ! grep -Eq '^CodeDirectory .*flags=.*runtime' <<< "$metadata" \
      || ! grep -q '^Timestamp=' <<< "$metadata"; then
      echo 'Expected Developer ID, team, hardened runtime and secure timestamp were not all present.' >&2
      exit 1
    fi
  done
  xcrun stapler validate "$candidate"
  spctl --assess --type execute --verbose=4 "$candidate"
}

verify_app "$app"
hdiutil verify "$dmg"
mount_dir=$(mktemp -d "${TMPDIR:-/tmp}/tidy-release-mount.XXXXXX")
mounted=0
cleanup() {
  if [[ "$mounted" = 1 ]]; then
    hdiutil detach "$mount_dir"
  fi
}
trap cleanup EXIT
hdiutil attach -readonly -nobrowse -mountpoint "$mount_dir" "$dmg"
mounted=1
verify_app "$mount_dir/Tidy.app"
# Includes resources and the stapled ticket, not just the executable.
diff -qr "$app" "$mount_dir/Tidy.app"
echo 'Verified Developer ID signing, notarisation and Gatekeeper acceptance for the app inside the DMG.'

#!/usr/bin/env bash
# Package the already-notarised app. Does not install or publish anything.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)
[[ $# -eq 2 ]] || { echo 'Usage: prepare-updater.sh NOTARISATION_DIRECTORY BUILD_DIRECTORY' >&2; exit 1; }
attempt=$(cd "$1" && pwd)
build=$(cd "$2" && pwd)
key="${TIDY_UPDATER_KEY_PATH:-$HOME/.tidy-signing/updater.key}"
[[ -f "$key" && -f "$key.pub" ]] || { echo 'Updater signing key is missing. See docs/RELEASING.md.' >&2; exit 1; }
[[ ! -e "$attempt/Tidy.app.tar.gz" && ! -e "$attempt/latest.json" ]] || { echo 'Updater files already exist; use a new notarisation attempt.' >&2; exit 1; }
# The signing key must match the public key compiled into this build.
node - "$build/source/src-tauri/tauri.conf.json" "$key.pub" <<'JS'
const fs=require('fs');
const config=JSON.parse(fs.readFileSync(process.argv[2],'utf8'));
if(config.plugins?.updater?.pubkey.trim() !== fs.readFileSync(process.argv[3],'utf8').trim()) throw new Error('Updater signing key does not match this app build.');
JS
codesign --verify --deep --strict "$attempt/Tidy.app"
xcrun stapler validate "$attempt/Tidy.app"
version=$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")).version' "$build/release.json")
app_version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$attempt/Tidy.app/Contents/Info.plist")
[[ "$app_version" = "$version" ]] || { echo 'App and release versions do not match.' >&2; exit 1; }
COPYFILE_DISABLE=1 tar -czf "$attempt/Tidy.app.tar.gz" -C "$attempt" Tidy.app
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" \
  npx tauri signer sign --private-key-path "$key" "$attempt/Tidy.app.tar.gz" > "$attempt/updater-signing.log"
node scripts/write-update-manifest.mjs "$attempt" "$version"
(
  cd "$build/source"
  CARGO_TARGET_DIR="$root/target" TIDY_UPDATE_FIXTURE="$attempt/Tidy.app.tar.gz" DYLD_FALLBACK_LIBRARY_PATH=/usr/lib/swift \
    cargo test -p tidy --lib --locked verify_real_update_signature -- --ignored > "$attempt/updater-verification.log" 2>&1
  grep -Fq 'test commands::updates::tests::verify_real_update_signature ... ok' "$attempt/updater-verification.log"
)
(
  cd "$attempt"
  shasum -a 256 Tidy.app.tar.gz Tidy.app.tar.gz.sig latest.json >> SHA256SUMS.txt
)
echo "Verified updater archive and manifest: $attempt"

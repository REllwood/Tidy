#!/usr/bin/env bash
# Notarise a local build using credentials stored in Keychain, then make its DMG.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)
if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo 'Usage: npm run release:notarise -- /path/to/build [keychain-profile]' >&2; exit 1;
fi
run_dir=$(cd "$1" && pwd)
profile="${2:-tidy-notary}"
[[ -f "$run_dir/release.json" && -d "$run_dir/Tidy.app" ]] || { echo 'Not a completed local build.' >&2; exit 1; }
version=$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).version' "$run_dir/release.json")
team=$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).team' "$run_dir/release.json")
identity=$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).identity' "$run_dir/release.json")
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ && "$team" =~ ^[A-Z0-9]{10}$ ]] || { echo 'Invalid release metadata.' >&2; exit 1; }
echo 'Checking notarisation credentials in Keychain...'
xcrun notarytool history --keychain-profile "$profile" --output-format json > /dev/null
# Keep each attempt separate; no previous app, DMG or submission log is removed.
attempt=$(mktemp -d "$run_dir/notarisation.XXXXXX")
app="$attempt/Tidy.app"
ditto "$run_dir/Tidy.app" "$app"
codesign --verify --deep --strict --verbose=2 "$app"

notarise() {
  local archive="$1" name="$2" submission_id
  echo "Submitting $name to Apple..."
  xcrun notarytool submit "$archive" --keychain-profile "$profile" \
    --output-format json > "$attempt/$name-submission.json"
  submission_id=$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).id' "$attempt/$name-submission.json")
  echo "Waiting for Apple: $submission_id (submission details saved in $attempt)"
  xcrun notarytool wait "$submission_id" --keychain-profile "$profile" \
    --timeout 30m --output-format json > "$attempt/$name-result.json"
  node - "$attempt/$name-result.json" <<'JS'
const fs = require('fs');
const result = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
if (result.status !== 'Accepted') throw new Error(`Apple did not accept this submission: ${result.status}. Retrieve its notarytool log before retrying.`);
JS
}

echo 'Packaging the signed app for Apple...'
ditto -c -k --keepParent "$app" "$attempt/Tidy.zip"
notarise "$attempt/Tidy.zip" app
xcrun stapler staple "$app"
xcrun stapler validate "$app"

mkdir "$attempt/dmg-content"
ditto "$app" "$attempt/dmg-content/Tidy.app"
ln -s /Applications "$attempt/dmg-content/Applications"
dmg="$attempt/Tidy_${version}_aarch64.dmg"
echo 'Creating the download DMG...'
hdiutil create -volname Tidy -srcfolder "$attempt/dmg-content" -format UDZO -ov "$dmg"
codesign --sign "$identity" --timestamp "$dmg"
notarise "$dmg" dmg
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
codesign --verify --strict --verbose=2 "$dmg"
spctl --assess --type open --context context:primary-signature --verbose=4 "$dmg"
APPLE_TEAM_ID="$team" bash "$root/scripts/verify-macos-release.sh" "$app" "$dmg"
(
  cd "$attempt"
  shasum -a 256 "Tidy_${version}_aarch64.dmg" > SHA256SUMS.txt
)
echo "Verified release download: $dmg"
echo 'Ready for upload to a GitHub release. Nothing has been uploaded or published.'

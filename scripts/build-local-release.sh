#!/usr/bin/env bash
# Build a distributable app on the maintainer's Mac. No upload or app launch.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)
[[ "$(uname -s)" = Darwin && "$(uname -m)" = arm64 ]] || {
  echo 'This release build requires an Apple Silicon Mac.' >&2; exit 1;
}
identity="${APPLE_SIGNING_IDENTITY:-Developer ID Application: Rhys Ellwood (3478YKFMRY)}"
team="${APPLE_TEAM_ID:-3478YKFMRY}"
[[ "$identity" = "Developer ID Application: "* && "$identity" = *" ($team)" ]] || {
  echo 'A matching Developer ID Application identity and team are required.' >&2; exit 1;
}
identities=$(security find-identity -v -p codesigning)
grep -Fq "\"$identity\"" <<< "$identities" || {
  echo 'The Developer ID Application certificate and private key are not available in Keychain.' >&2; exit 1;
}
version=$(node -p 'require("./src-tauri/tauri.conf.json").version')
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'Invalid release version.' >&2; exit 1; }
mkdir -p "$root/release-builds"
run_dir=$(mktemp -d "$root/release-builds/Tidy-$version.XXXXXX")
mkdir "$run_dir/source"
echo "Build directory: $run_dir"
# Build a fresh source snapshot so npm ci and Vite cannot remove workspace files.
# git supplies only tracked/non-ignored paths, excluding signing material and caches.
git ls-files --cached --others --exclude-standard -z -- . ':(exclude).forge/**' > "$run_dir/source-files.txt"
rsync -a --from0 --files-from="$run_dir/source-files.txt" "$root/" "$run_dir/source/"
git rev-parse HEAD > "$run_dir/source-commit.txt"
git status --porcelain=v1 > "$run_dir/source-status.txt"
export CARGO_TARGET_DIR="$root/target"
(
  cd "$run_dir/source"
  echo 'Installing locked dependencies and running tests...'
  npm ci --no-audit --no-fund
  npm test
  cargo test -p tidy-core --locked
  bash scripts/build-sidecar.sh
  TIDY_AI_BUILD_CACHE="$root/target/tidy-ai-runtime" bash scripts/build-ai-runtime.sh
  echo 'Building and signing Tidy with the certificate in Keychain...'
  # Notarisation is a separate step using a Keychain profile; no exported key or
  # Apple password is passed to Tauri or stored in the source snapshot.
  env -u APPLE_CERTIFICATE -u APPLE_CERTIFICATE_PASSWORD -u APPLE_ID \
    -u APPLE_PASSWORD -u APPLE_TEAM_ID -u APPLE_API_KEY -u APPLE_API_ISSUER \
    -u APPLE_API_KEY_PATH APPLE_SIGNING_IDENTITY="$identity" \
    npm run tauri build -- --ci --bundles app -- --locked
)
ditto "$CARGO_TARGET_DIR/release/bundle/macos/Tidy.app" "$run_dir/Tidy.app"
codesign --verify --deep --strict --verbose=2 "$run_dir/Tidy.app"
metadata=$(codesign -d --verbose=4 "$run_dir/Tidy.app" 2>&1)
grep -Fqx "TeamIdentifier=$team" <<< "$metadata"
grep -q '^Authority=Developer ID Application:' <<< "$metadata"
node - "$run_dir" "$version" "$team" "$identity" <<'JS'
const fs = require('fs');
const [dir, version, team, identity] = process.argv.slice(2);
fs.writeFileSync(`${dir}/release.json`, JSON.stringify({ version, team, identity, architecture: 'arm64', builtAt: new Date().toISOString() }, null, 2) + '\n');
JS
echo "Signed app built: $run_dir/Tidy.app"
echo "Next: npm run release:notarise -- '$run_dir' tidy-notary"
echo 'The app has not been notarised or uploaded yet.'

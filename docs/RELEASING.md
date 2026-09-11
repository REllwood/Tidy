# Local macOS releases

Build, sign and notarise Tidy on the maintainer's Mac. GitHub hosts the finished DMG;
it does not build the app. A published release is visible to everyone on the repository's
Releases page. A draft is visible only to users who can manage releases.

## One-time setup

The signing identity already available in Keychain is:

`Developer ID Application: Rhys Ellwood (3478YKFMRY)`

No `.p12` export or GitHub signing secrets are needed. On this Mac, save notarisation
credentials with the following interactive command:

```bash
xcrun notarytool store-credentials tidy-notary --team-id 3478YKFMRY
```

Enter the Apple Account email and an app-specific password when prompted. The password
is stored in Keychain. Create it in your Apple Account's Sign-In and Security settings;
do not use your normal account password or paste any password into chat or source files.
If you already have a suitable Keychain profile, use that name instead of `tidy-notary`.

For uploading from Terminal, sign in once:

```bash
gh auth login
```

Alternatively, upload the finished DMG through GitHub's browser interface.

## Build and notarise

Keep the version consistent in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`,
`Cargo.lock` and `src-tauri/tauri.conf.json`. Choose a new version for each release so existing
downloads are not overwritten.

```bash
npm run release:build
```

This creates a fresh source snapshot under ignored `release-builds/`, installs locked
Node dependencies, runs front-end and core tests, builds the MCP sidecar and signs the
app with the identity in Keychain. It does not launch or install Tidy. It records the
source commit and working-tree status; any uncommitted source still needs review before
public distribution. Build caches are reused, and earlier build directories are retained.

Use the exact directory printed at the end of the build:

```bash
npm run release:notarise -- /absolute/path/to/release-builds/Tidy-0.3.0.XXXXXX tidy-notary
```

The script notarises and staples the app first, creates a DMG with an Applications
shortcut, then signs, notarises and staples that DMG. It checks signature integrity,
Developer ID, team, hardened runtime, timestamps, architecture, notarisation tickets and
Gatekeeper acceptance, including the copy of Tidy inside the finished DMG. It writes a
SHA256SUMS.txt alongside the verified download. Passwords stay in Keychain.

Apple can take longer for an initial submission. Each attempt retains submission IDs and
results. If the 30-minute wait expires, the submission continues at Apple; use
`xcrun notarytool wait SUBMISSION_ID --keychain-profile tidy-notary` to inspect it before
submitting again. No failed or incomplete attempt is automatically uploaded.

## GitHub release

Once the source/version tag and finished download have been reviewed, upload the exact
verified DMG and its SHA256SUMS.txt through the repository's New Release page. Use a new
`v0.3.0` tag tied to the matching source, not an unrelated commit. Save it as a draft for a
final download/launch check on a separate Mac, then publish it to make it public.

The equivalent upload command, once a matching tag already exists on GitHub, is:

```bash
gh release create v0.3.0 /absolute/path/Tidy_0.3.0_aarch64.dmg /absolute/path/SHA256SUMS.txt --repo REllwood/Tidy --verify-tag --draft --title "Tidy v0.3.0" --notes-file /absolute/path/release-notes.md
```

This command creates a draft, refuses a missing tag and does not overwrite an existing
release. Publishing, creating tags and committing source changes remain explicit release
actions; the build scripts never perform them. The local GitHub workflow file is manual-only and shows these instructions. The old
workflow on GitHub has been disabled, so tag pushes do not start another build. The workflow is instructions-only.

## Compatibility

The current build targets Apple Silicon. The configured minimum macOS version is 14.4;
test that version separately before claiming support. Intel, Windows and Linux are not
included. Signing fixes distribution trust, not application bugs or OS compatibility.

Version 0.1.3 was the first public signed replacement. Version 0.1.2 remains available as an older,
ad-hoc signed release.

For v0.1.3, no source commit was created. Its tag refers to base commit
`041879a9fad187770b25fc8abd9af593d7eb1b57`. The release includes
`Tidy_0.1.3_source.tar.gz` with the actual build snapshot and its checksum; use that
attachment rather than GitHub's automatic source archive to reproduce this build.
The README and this guide were updated locally after packaging and are not part of
that snapshot.

References: [Tauri macOS signing](https://v2.tauri.app/distribute/sign/macos/) and
[Apple notarisation workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).


## Builds with managed local AI

The build now also compiles the pinned inference runtime with
`scripts/build-ai-runtime.sh` and stages it beside the MCP executable. The runtime
must have no Homebrew or build-directory dynamic dependencies. Release verification
checks Developer ID signing on all three executables. See [Local AI](LOCAL-AI.md)
for model downloads, synthetic inference tests and current product limitations.
Version 0.2.0 introduces managed AI, client-scoped meeting questions and progress reporting.
For this and subsequent releases, commit the reviewed source and tag that exact commit
before publishing. The build excludes local `.forge` working notes.

## In-app updates

Version 0.3.0 adds **Check for updates** in Settings and a prompt when a
new version is available. Users choose whether to download and install it. Tidy
saves pending editor changes and restarts after installation. Recording and
transcription block installation. Automatic checks run after opening and every
six hours, using GitHub; meeting content is not sent.

People using v0.2.1 need to install the first updater-enabled release manually.
The updater cannot be added remotely to a version that does not include it.

Updater signatures are separate from Apple signing. The public key is in
`src-tauri/tauri.conf.json`; the private key on the maintainer's Mac is at
`~/.tidy-signing/updater.key`. Back up that key securely. Do not commit or share it.
Keep using the same key for future releases. `TIDY_UPDATER_KEY_PATH` can select a
backup key location, with the matching public key in the adjacent `.pub` file.

After notarisation and stapling, `prepare-updater.sh` creates and signs
`Tidy.app.tar.gz`, verifies its signature (including rejection of changed bytes),
and writes `latest.json`. Packaging happens after stapling so the downloaded app
contains its notarisation ticket. The script checks that the key matches the app's
source snapshot and does not overwrite an earlier attempt.

Upload these files to the same versioned GitHub release:

- The verified DMG and `SHA256SUMS.txt`
- `Tidy.app.tar.gz` and `Tidy.app.tar.gz.sig`
- `latest.json`

The manifest points to the archive under that exact release tag. Publish only once
all files are uploaded. It currently includes Apple Silicon only. A missing feed,
failed download or invalid signature produces an error; it is not reported as an
up-to-date result. Nothing is installed automatically in the browser preview.

The release checks exercise signature verification and the actual updater installer
against an isolated app fixture. A full upgrade between running app versions on a
separate Mac, including the Applications permission prompt and restart, remains
an additional manual check. The fixture test does not launch the app or open a
user workspace.

Reference: [Tauri updater](https://v2.tauri.app/plugin/updater/).

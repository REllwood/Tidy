#!/usr/bin/env bash
# Validate presence and identity without printing credential values.
set -euo pipefail

missing=0
for name in APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID; do
  if [[ -z "${!name:-}" ]]; then
    echo "::error::Missing GitHub Actions secret: $name"
    missing=1
  fi
done
if [[ "$missing" != 0 ]]; then
  exit 1
fi

if [[ ! "$APPLE_TEAM_ID" =~ ^[A-Z0-9]{10}$ ]]; then
  echo '::error::APPLE_TEAM_ID must be the ten-character Apple Developer team ID.'
  exit 1
fi
if [[ "$APPLE_SIGNING_IDENTITY" != "Developer ID Application: "* || "$APPLE_SIGNING_IDENTITY" != *" ($APPLE_TEAM_ID)" ]]; then
  echo '::error::Use a Developer ID Application identity belonging to APPLE_TEAM_ID.'
  exit 1
fi
echo 'Signing configuration is present. The build will validate the certificate and Apple credentials.'

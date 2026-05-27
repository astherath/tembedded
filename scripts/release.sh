#!/usr/bin/env bash
#
# Build the firmware in release mode and publish it + an updated manifest to
# the public `firmware` container on the `binsbucket` storage account. Uses
# your signed-in az CLI identity via --auth-mode login.
#
# Prereqs (one-time, see README "Storage setup"):
#   * az login
#   * storage account `binsbucket`, container `firmware` with public blob read
#   * your identity has "Storage Blob Data Contributor" on the storage account
#
# Usage:
#   . ~/export-esp.sh   # esp toolchain env
#   ./scripts/release.sh 0.2.0

set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <version>   e.g. $0 0.2.0" >&2
  exit 1
fi

VERSION="$1"
# Match azure_setup.sh — override via env vars when forking.
ACCOUNT="${AZ_STORAGE_ACCOUNT:-binsbucket}"
CONTAINER="${AZ_CONTAINER:-firmware}"
ELF_PATH="target/xtensa-esp32s3-espidf/release/tembedded"
BIN_NAME="tembedded-${VERSION}.bin"
BIN_PATH="target/xtensa-esp32s3-espidf/release/${BIN_NAME}"
MANIFEST_PATH="target/xtensa-esp32s3-espidf/release/manifest.json"

echo "===> sanity-check: CONFIG_APP_PROJECT_VER and ota::CURRENT_VERSION match v${VERSION}"
SDKVER=$(grep '^CONFIG_APP_PROJECT_VER=' sdkconfig.defaults | sed 's/.*="\(.*\)"/\1/')
SRCVER=$(grep 'pub const CURRENT_VERSION' src/ota.rs | sed 's/.*"\(.*\)".*/\1/')
if [[ "$SDKVER" != "$VERSION" ]] || [[ "$SRCVER" != "$VERSION" ]]; then
  echo "  ✗ version mismatch: sdkconfig=$SDKVER, src/ota.rs=$SRCVER, arg=$VERSION" >&2
  echo "  bump both before releasing." >&2
  exit 1
fi

echo "===> cargo build --release"
cargo build --release

echo "===> espflash save-image -> ${BIN_NAME}"
espflash save-image --chip esp32s3 --flash-size 16mb "${ELF_PATH}" "${BIN_PATH}"

echo "===> writing manifest.json"
cat > "${MANIFEST_PATH}" <<EOF
{
  "version": "${VERSION}",
  "url": "https://${ACCOUNT}.blob.core.windows.net/${CONTAINER}/${BIN_NAME}"
}
EOF
cat "${MANIFEST_PATH}"

echo "===> uploading ${BIN_NAME} ($(wc -c < "${BIN_PATH}") bytes) + manifest.json in parallel"
# Run both uploads concurrently. The binary blob is the slow one; the
# manifest is tiny so it'll finish first and we just wait on both.
az storage blob upload \
  --account-name "${ACCOUNT}" \
  --container-name "${CONTAINER}" \
  --name "${BIN_NAME}" \
  --file "${BIN_PATH}" \
  --overwrite \
  --auth-mode login \
  --output none &
PID_BIN=$!

az storage blob upload \
  --account-name "${ACCOUNT}" \
  --container-name "${CONTAINER}" \
  --name "manifest.json" \
  --file "${MANIFEST_PATH}" \
  --overwrite \
  --content-cache "no-cache, no-store, must-revalidate" \
  --auth-mode login \
  --output none &
PID_MAN=$!

# Fail the whole release if either upload errors out.
wait $PID_BIN || { echo "  ✗ firmware upload failed" >&2; exit 1; }
wait $PID_MAN || { echo "  ✗ manifest upload failed" >&2; exit 1; }

echo
echo "✓ published v${VERSION}"
echo "  manifest: https://${ACCOUNT}.blob.core.windows.net/${CONTAINER}/manifest.json"
echo "  binary:   https://${ACCOUNT}.blob.core.windows.net/${CONTAINER}/${BIN_NAME}"

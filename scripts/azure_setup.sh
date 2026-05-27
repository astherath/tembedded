#!/usr/bin/env bash
#
# One-time Azure setup for the OTA bucket. Idempotent — safe to re-run.
#
# After this finishes the `binsbucket` storage account has:
#   * a `firmware` container that allows anonymous public reads of its blobs
#   * your signed-in identity granted "Storage Blob Data Contributor" on the
#     account, so scripts/release.sh can upload with --auth-mode login
#
# Prereqs:
#   * az login
#   * you have Owner or User Access Administrator on the storage account
#     (needed for the role assignment step)

set -euo pipefail

# Storage targets. Override via env vars when forking this project; defaults
# match the bucket this repo was originally pointed at.
ACCOUNT="${AZ_STORAGE_ACCOUNT:-binsbucket}"
RG="${AZ_RESOURCE_GROUP:-claudisplay}"
CONTAINER="${AZ_CONTAINER:-firmware}"

echo "===> sanity-check: az session"
SUBSCRIPTION_ID=$(az account show --query id -o tsv)
USER_OID=$(az ad signed-in-user show --query id -o tsv)
echo "  subscription: ${SUBSCRIPTION_ID}"
echo "  identity:     ${USER_OID}"

echo "===> allow public blob access on account (account-level toggle)"
az storage account update \
  --name "${ACCOUNT}" \
  --resource-group "${RG}" \
  --allow-blob-public-access true \
  --output none

echo "===> grant 'Storage Blob Data Contributor' on the account to ${USER_OID}"
SCOPE=$(az storage account show -n "${ACCOUNT}" -g "${RG}" --query id -o tsv)
az role assignment create \
  --assignee-object-id "${USER_OID}" \
  --assignee-principal-type User \
  --role "Storage Blob Data Contributor" \
  --scope "${SCOPE}" \
  --output none || echo "  (role assignment already exists, continuing)"

echo "===> waiting briefly for RBAC propagation..."
sleep 15

echo "===> create container ${CONTAINER} with public-blob anonymous read"
az storage container create \
  --account-name "${ACCOUNT}" \
  --name "${CONTAINER}" \
  --public-access blob \
  --auth-mode login \
  --output none

echo
echo "✓ setup complete."
echo "  Container URL: https://${ACCOUNT}.blob.core.windows.net/${CONTAINER}/"
echo "  Test once a binary is uploaded:"
echo "    curl -I https://${ACCOUNT}.blob.core.windows.net/${CONTAINER}/manifest.json"

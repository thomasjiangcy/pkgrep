#!/usr/bin/env bash
set -euo pipefail

container="${1:-pkgrep-cache}"
project="${COMPOSE_PROJECT_NAME:-pkgrep-azurite}"
network="${project}_default"
account_name="${AZURE_ACCOUNT_NAME:-devstoreaccount1}"
account_key="${AZURE_ACCOUNT_KEY:-Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==}"
endpoint="${AZURE_BLOB_ENDPOINT:-http://azurite:10000/${account_name}}"
az_image="${AZURE_CLI_IMAGE:-mcr.microsoft.com/azure-cli:2.73.0}"

connection_string="DefaultEndpointsProtocol=http;AccountName=${account_name};AccountKey=${account_key};BlobEndpoint=${endpoint};"

echo "Ensuring Azure Blob container '${container}' exists via ${endpoint} on docker network ${network}"

for _ in $(seq 1 60); do
  if docker run --rm --network "${network}" "${az_image}" \
    az storage container create \
    --name "${container}" \
    --connection-string "${connection_string}" \
    --only-show-errors \
    --output none >/dev/null 2>&1; then
    echo "Container ready: ${container}"
    exit 0
  fi

  sleep 1
done

echo "Failed to ensure container exists: ${container}" >&2
exit 1

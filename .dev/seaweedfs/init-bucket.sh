#!/usr/bin/env bash
set -euo pipefail

bucket="${1:-pkgrep-cache}"
project="${COMPOSE_PROJECT_NAME:-pkgrep-seaweed}"
network="${project}_default"
endpoint="${S3_ENDPOINT:-http://seaweedfs:8333}"
access_key="${AWS_ACCESS_KEY_ID:-pkgrep}"
secret_key="${AWS_SECRET_ACCESS_KEY:-pkgrepsecret}"
aws_image="${AWS_CLI_IMAGE:-amazon/aws-cli:2.31.17}"

echo "Ensuring S3 bucket '${bucket}' exists via ${endpoint} on docker network ${network}"

for _ in $(seq 1 60); do
  if docker run --rm --network "${network}" \
    -e AWS_ACCESS_KEY_ID="${access_key}" \
    -e AWS_SECRET_ACCESS_KEY="${secret_key}" \
    "${aws_image}" \
    s3api head-bucket --bucket "${bucket}" --endpoint-url "${endpoint}" >/dev/null 2>&1; then
    echo "Bucket already exists: ${bucket}"
    exit 0
  fi

  if docker run --rm --network "${network}" \
    -e AWS_ACCESS_KEY_ID="${access_key}" \
    -e AWS_SECRET_ACCESS_KEY="${secret_key}" \
    "${aws_image}" \
    s3api create-bucket --bucket "${bucket}" --endpoint-url "${endpoint}" --region us-east-1 >/dev/null 2>&1; then
    echo "Bucket created: ${bucket}"
    exit 0
  fi

  sleep 1
done

echo "Failed to ensure bucket exists: ${bucket}" >&2
exit 1

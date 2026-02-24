#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

failed=0

check_pattern() {
  local label="$1"
  local pattern="$2"
  shift 2
  local paths=("$@")

  if rg -n --color=never -e "$pattern" "${paths[@]}"; then
    echo "error: found forbidden mocking/stubbing pattern ($label)"
    failed=1
  fi
}

check_dirs() {
  local found=0
  while IFS= read -r dir; do
    echo "$dir"
    found=1
  done < <(find src tests -type d \( -name mocks -o -name stubs -o -name __mocks__ \) 2>/dev/null || true)

  if [[ "$found" -eq 1 ]]; then
    echo "error: found forbidden mocking/stubbing directory names"
    failed=1
  fi
}

# Forbid common mocking libraries.
check_pattern "mocking libraries in Cargo.toml" "(mockall|mockito|wiremock|httpmock|galvanic-assert|double)" "Cargo.toml"

# Forbid common mocking/stubbing APIs and annotations in code/tests.
check_pattern "mocking/stubbing APIs" "(::mockall::|mockall::|mockito::|wiremock::|httpmock::|#\\[automock\\]|mock!\\s*\\{|MockServer|Mock::new\\(|Stub::new\\(|Fake::new\\()" "src" "tests"

check_dirs

if [[ "$failed" -ne 0 ]]; then
  exit 1
fi

echo "ok: no mocking/stubbing frameworks or patterns detected"

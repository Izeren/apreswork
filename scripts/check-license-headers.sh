#!/bin/bash
# Copyright 2026 Aleksandr Iushmanov (@izeren)
# SPDX-License-Identifier: Apache-2.0

# Check that all source files have SPDX license headers.
# Usage: ./scripts/check-license-headers.sh

set -euo pipefail

errors=0

check_header() {
  local file="$1"
  local pattern="$2"
  if ! head -5 "$file" | grep -q "$pattern"; then
    echo "MISSING HEADER: $file"
    errors=$((errors + 1))
  fi
}

# Rust files
while IFS= read -r -d '' file; do
  check_header "$file" "SPDX-License-Identifier: Apache-2.0"
done < <(find src-tauri/src -name '*.rs' -print0)

# TypeScript files
while IFS= read -r -d '' file; do
  check_header "$file" "SPDX-License-Identifier: Apache-2.0"
done < <(find src -name '*.ts' -print0 2>/dev/null || true)

# Svelte files
while IFS= read -r -d '' file; do
  check_header "$file" "SPDX-License-Identifier: Apache-2.0"
done < <(find src -name '*.svelte' -print0 2>/dev/null || true)

# CSS files (only in src/, not node_modules)
while IFS= read -r -d '' file; do
  check_header "$file" "SPDX-License-Identifier: Apache-2.0"
done < <(find src -name '*.css' -print0 2>/dev/null || true)

if [ "$errors" -gt 0 ]; then
  echo ""
  echo "ERROR: $errors file(s) missing SPDX license headers."
  echo "Run ./scripts/add-license-headers.sh to fix."
  exit 1
fi

echo "All source files have license headers."

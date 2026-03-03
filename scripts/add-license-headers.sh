#!/bin/bash
# Copyright 2026 Aleksandr Iushmanov (@izeren)
# SPDX-License-Identifier: Apache-2.0

# Add SPDX license headers to source files that are missing them.
# Usage: ./scripts/add-license-headers.sh

set -euo pipefail

YEAR="2026"
HOLDER="Aleksandr Iushmanov (@izeren)"

add_header_slash() {
  local file="$1"
  if ! head -5 "$file" | grep -q "SPDX-License-Identifier: Apache-2.0"; then
    local tmp
    tmp=$(mktemp)
    printf "// Copyright %s %s\n// SPDX-License-Identifier: Apache-2.0\n\n" "$YEAR" "$HOLDER" > "$tmp"
    cat "$file" >> "$tmp"
    mv "$tmp" "$file"
    echo "ADDED: $file"
  fi
}

add_header_html() {
  local file="$1"
  if ! head -5 "$file" | grep -q "SPDX-License-Identifier: Apache-2.0"; then
    local tmp
    tmp=$(mktemp)
    printf "<!-- Copyright %s %s -->\n<!-- SPDX-License-Identifier: Apache-2.0 -->\n\n" "$YEAR" "$HOLDER" > "$tmp"
    cat "$file" >> "$tmp"
    mv "$tmp" "$file"
    echo "ADDED: $file"
  fi
}

add_header_css() {
  local file="$1"
  if ! head -5 "$file" | grep -q "SPDX-License-Identifier: Apache-2.0"; then
    local tmp
    tmp=$(mktemp)
    printf "/* Copyright %s %s */\n/* SPDX-License-Identifier: Apache-2.0 */\n\n" "$YEAR" "$HOLDER" > "$tmp"
    cat "$file" >> "$tmp"
    mv "$tmp" "$file"
    echo "ADDED: $file"
  fi
}

# Rust files
find src-tauri/src -name '*.rs' -print0 | while IFS= read -r -d '' file; do
  add_header_slash "$file"
done

# TypeScript files
find src -name '*.ts' -print0 2>/dev/null | while IFS= read -r -d '' file; do
  add_header_slash "$file"
done || true

# Svelte files
find src -name '*.svelte' -print0 2>/dev/null | while IFS= read -r -d '' file; do
  add_header_html "$file"
done || true

# CSS files
find src -name '*.css' -print0 2>/dev/null | while IFS= read -r -d '' file; do
  add_header_css "$file"
done || true

echo "Done."

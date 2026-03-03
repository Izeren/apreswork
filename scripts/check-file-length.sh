#!/bin/bash
# Copyright 2026 Aleksandr Iushmanov (@izeren)
# SPDX-License-Identifier: Apache-2.0

# Check that source files do not exceed the maximum line count. Long files are a
# signal to split into smaller, focused modules. There are no exceptions: any
# in-scope file over the limit fails the check.
#
# Usage:
#   ./scripts/check-file-length.sh         # staged added/modified files only
#   ./scripts/check-file-length.sh --all   # every source file

set -euo pipefail

MAX_LINES=1000

violations=0

# Source scope mirrors the rest of the tooling (license headers, lefthook
# globs): Rust under src-tauri/src, TS/Svelte/CSS under src.
in_scope() {
  case "$1" in
  src-tauri/src/*.rs) return 0 ;;
  src/*.ts | src/*.svelte | src/*.css) return 0 ;;
  *) return 1 ;;
  esac
}

check_length() {
  local file="$1"
  in_scope "$file" || return 0
  [ -f "$file" ] || return 0
  local lines
  lines=$(wc -l <"$file")
  if [ "$lines" -gt "$MAX_LINES" ]; then
    echo "TOO LONG ($lines lines): $file"
    violations=$((violations + 1))
  fi
}

files=()
if [ "${1:-}" = "--all" ]; then
  while IFS= read -r -d '' file; do
    files+=("$file")
  done < <(find src-tauri/src -name '*.rs' -print0)
  while IFS= read -r -d '' file; do
    files+=("$file")
  done < <(find src \( -name '*.ts' -o -name '*.svelte' -o -name '*.css' \) -print0 2>/dev/null || true)
else
  # Files staged for this commit: Added, Copied, Modified, Renamed.
  while IFS= read -r file; do
    [ -n "$file" ] && files+=("$file")
  done < <(git diff --cached --name-only --diff-filter=ACMR)
fi

if [ "${#files[@]}" -gt 0 ]; then
  for file in "${files[@]}"; do
    check_length "$file"
  done
fi

if [ "$violations" -gt 0 ]; then
  echo ""
  echo "ERROR: $violations file(s) exceed $MAX_LINES lines. Split them into smaller modules."
  exit 1
fi

echo "All checked files are within $MAX_LINES lines."

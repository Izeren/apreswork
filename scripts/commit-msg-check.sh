#!/bin/bash
# Copyright 2026 Aleksandr Iushmanov (@izeren)
# SPDX-License-Identifier: Apache-2.0

# Validate conventional commit format and subject line length.
# Used by lefthook commit-msg hook.

set -euo pipefail

first_line=$(head -n 1 "$1")

# Allow merge commits
if [[ "$first_line" =~ ^Merge\  ]]; then
  exit 0
fi

# Check conventional commit format
pattern='^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\([a-zA-Z0-9_.-]+\))?!?: .+'
if ! [[ "$first_line" =~ $pattern ]]; then
  echo "ERROR: Commit message must follow Conventional Commits format."
  echo "  Format:  type(scope)?: description"
  echo "  Types:   feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert"
  echo "  Example: feat(scheduler): add slot collision detection"
  echo ""
  echo "  Got: $first_line"
  exit 1
fi

# Check subject length (max 72 characters)
if [ ${#first_line} -gt 72 ]; then
  echo "ERROR: Commit subject must be ≤72 characters (currently ${#first_line})."
  echo "  Got: $first_line"
  exit 1
fi

exit 0

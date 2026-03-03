#!/bin/bash
# Copyright 2026 Aleksandr Iushmanov (@izeren)
# SPDX-License-Identifier: Apache-2.0

# Remove stale frontend artifacts (vite dep-optimization cache, build output).
# Stale node_modules/.vite has repeatedly broken `tauri dev` with
# vite-plugin-svelte virtual-module load failures. Run `npm run clean` before
# `npx tauri dev` when that happens. Idempotent.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

rm -rf "$REPO_ROOT/node_modules/.vite" "$REPO_ROOT/build"

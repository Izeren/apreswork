#!/bin/bash
# Copyright 2026 Aleksandr Iushmanov (@izeren)
# SPDX-License-Identifier: Apache-2.0

# Run code coverage checks with threshold enforcement.
# Requires: cargo-llvm-cov, nightly toolchain with llvm-tools-preview
#
# Usage:
#   bash scripts/coverage.sh              # print summary to stdout
#   bash scripts/coverage.sh -o result    # write final status line to result file

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COVERAGE_BASELINE_FILE="${COVERAGE_BASELINE_FILE:-$REPO_ROOT/scripts/coverage-baseline.env}"
CHANGED_CODE_MIN="${CHANGED_CODE_MIN:-90}"
COVERAGE_DIFF_BASE="${COVERAGE_DIFF_BASE:-HEAD}"
# Colon-separated list of src-tauri/src path suffixes to exclude from the
# changed-code coverage gate.  Use for Tauri glue files whose lines execute
# only inside the Tauri runtime and cannot be reached by unit tests.
# Example: "src/lib.rs:src/main.rs"
COVERAGE_CHANGED_IGNORE="${COVERAGE_CHANGED_IGNORE:-src-tauri/src/lib.rs}"
# Rust files excluded from coverage measurement entirely: the Tauri glue
# layer (lib.rs run()/registration, the main.rs shim, and command thin
# wrappers) executes only inside the Tauri runtime and is exempt from tests
# per the CLAUDE.md testing rules, so keeping it in the measured set only
# dilutes the totals the ratchet compares.
COVERAGE_IGNORE_REGEX="${COVERAGE_IGNORE_REGEX:-src-tauri/src/(commands/|lib\.rs|main\.rs)}"

output_file=""
while getopts "o:" opt; do
  case $opt in
    o) output_file="$OPTARG" ;;
    *) echo "Usage: $0 [-o output_file]" >&2; exit 1 ;;
  esac
done

report() {
  echo "$1"
  if [[ -n "$output_file" ]]; then
    echo "$1" >> "$output_file"
  fi
}

percent_lt() {
  awk -v lhs="$1" -v rhs="$2" 'BEGIN { exit !((lhs + 0) < (rhs + 0)) }'
}

format_percent() {
  awk -v numerator="$1" -v denominator="$2" 'BEGIN {
    if ((denominator + 0) == 0) {
      print "n/a"
      exit
    }

    printf "%.2f", (numerator / denominator) * 100
  }'
}

extract_total_percent() {
  local summary_path="$1"
  local metric="$2"

  awk -v metric="$metric" '
    /^TOTAL[[:space:]]/ {
      if (metric == "regions") {
        gsub(/%/, "", $4)
        print $4
      } else if (metric == "lines") {
        gsub(/%/, "", $10)
        print $10
      } else if (metric == "branches") {
        gsub(/%/, "", $13)
        print $13
      }
    }
  ' "$summary_path"
}

generate_branch_gap_report() {
  local lcov_path="$1"
  local report_path="$2"
  local tmp_details
  tmp_details="$(mktemp)"

  awk -F: '
    /^SF:/ {
      file = substr($0, 4)
      next
    }
    /^BRDA:/ {
      split(substr($0, 6), parts, ",")
      line = parts[1]
      block = parts[2]
      branch = parts[3]
      hits = parts[4]
      if (hits == "-" || hits == "0") {
        printf "%s\t%s\t%s\t%s\t%s\n", file, line, branch, block, hits
      }
    }
  ' "$lcov_path" > "$tmp_details"

  {
    echo "# Branch Gap Report"
    echo
    echo "Source: $lcov_path"
    echo

    local branch_hit branch_total
    read -r branch_hit branch_total < <(
      awk -F: '
        /^BRH:/ { hit += substr($0, 5) }
        /^BRF:/ { total += substr($0, 5) }
        END { printf "%d %d\n", hit, total }
      ' "$lcov_path"
    )

    local branch_pct="0.00"
    if [[ "${branch_total:-0}" -gt 0 ]]; then
      branch_pct="$(awk -v hit="$branch_hit" -v total="$branch_total" 'BEGIN { printf "%.2f", (hit / total) * 100 }')"
    fi

    echo "Overall branch coverage: ${branch_pct}% (${branch_hit:-0}/${branch_total:-0})"
    echo

    if [[ ! -s "$tmp_details" ]]; then
      echo "No uncovered branches found."
    else
      echo "## Summary By File"
      awk -F'\t' '{ counts[$1]++ } END { for (file in counts) printf "%6d  %s\n", counts[file], file }' "$tmp_details" | sort -nr
      echo
      echo "## Uncovered Branches"
      sort -t$'\t' -k1,1 -k2,2n -k3,3n "$tmp_details" | awk -F'\t' '{
        printf "%s:%s branch=%s block=%s hits=%s\n", $1, $2, $3, $4, $5
      }'
    fi
  } > "$report_path"

  rm -f "$tmp_details"
}

generate_changed_code_report() {
  local diff_base="$1"
  local lcov_path="$2"
  local report_path="$3"
  local diff_output changed_lines counts_file file_summary missing_lines missing_branches

  diff_output="$(mktemp)"
  changed_lines="$(mktemp)"
  counts_file="$(mktemp)"
  file_summary="$(mktemp)"
  missing_lines="$(mktemp)"
  missing_branches="$(mktemp)"

  git -C "$REPO_ROOT" diff --unified=0 --no-color --diff-filter=AM "$diff_base" -- src-tauri/src > "$diff_output"

  # Build a newline-separated ignore list from the colon-separated variable.
  local ignore_list
  ignore_list="$(echo "${COVERAGE_CHANGED_IGNORE:-}" | tr ':' '\n')"

  awk -v root="$REPO_ROOT" -v ignore_raw="$ignore_list" '
    BEGIN {
      n = split(ignore_raw, ignores, "\n")
    }
    function is_ignored(path,    i) {
      for (i = 1; i <= n; i++) {
        if (ignores[i] != "" && index(path, ignores[i]) > 0) {
          return 1
        }
      }
      return 0
    }
    /^diff --git / {
      file = ""
      next
    }
    /^\+\+\+ b\// {
      file = substr($0, 7)
      next
    }
    /^@@ / {
      if (file == "" || file == "/dev/null" || is_ignored(file)) {
        next
      }

      split($0, parts, " ")
      new_range = parts[3]
      sub(/^\+/, "", new_range)
      split(new_range, numbers, ",")
      start = numbers[1] + 0
      count = (numbers[2] == "" ? 1 : numbers[2] + 0)

      if (count == 0) {
        next
      }

      for (i = 0; i < count; i++) {
        printf "%s/%s\t%d\n", root, file, start + i
      }
    }
  ' "$diff_output" > "$changed_lines"

  awk -F: \
    -v counts_path="$counts_file" \
    -v file_summary_path="$file_summary" \
    -v missing_lines_path="$missing_lines" \
    -v missing_branches_path="$missing_branches" '
    FNR == NR {
      split($0, parts, "\t")
      changed[parts[1] SUBSEP parts[2]] = 1
      changed_files[parts[1]] = 1
      next
    }
    /^SF:/ {
      file = substr($0, 4)
      next
    }
    /^DA:/ {
      split(substr($0, 4), parts, ",")
      line = parts[1]
      hits = parts[2] + 0
      key = file SUBSEP line
      if (!(key in changed)) {
        next
      }

      line_total++
      file_line_total[file]++
      if (hits > 0) {
        line_hit++
        file_line_hit[file]++
      } else {
        printf "%s\t%s\t%s\n", file, line, hits >> missing_lines_path
      }
      next
    }
    /^BRDA:/ {
      split(substr($0, 6), parts, ",")
      line = parts[1]
      block = parts[2]
      branch = parts[3]
      hits = parts[4]
      key = file SUBSEP line
      if (!(key in changed)) {
        next
      }

      branch_total++
      file_branch_total[file]++
      if (hits != "-" && (hits + 0) > 0) {
        branch_hit++
        file_branch_hit[file]++
      } else {
        printf "%s\t%s\t%s\t%s\t%s\n", file, line, branch, block, hits >> missing_branches_path
      }
      next
    }
    END {
      printf "%d %d %d %d\n", line_hit, line_total, branch_hit, branch_total > counts_path
      for (file in changed_files) {
        printf "%s\t%d\t%d\t%d\t%d\n",
          file,
          file_line_hit[file] + 0,
          file_line_total[file] + 0,
          file_branch_hit[file] + 0,
          file_branch_total[file] + 0 >> file_summary_path
      }
    }
  ' "$changed_lines" "$lcov_path"

  local line_hit line_total branch_hit branch_total
  read -r line_hit line_total branch_hit branch_total < "$counts_file"

  local line_pct branch_pct
  line_pct="$(format_percent "${line_hit:-0}" "${line_total:-0}")"
  branch_pct="$(format_percent "${branch_hit:-0}" "${branch_total:-0}")"

  {
    echo "# Changed Code Coverage"
    echo
    echo "Diff base: $diff_base"
    echo "Source: $lcov_path"
    echo

    if [[ "${line_total:-0}" -eq 0 && "${branch_total:-0}" -eq 0 ]]; then
      echo "No changed executable Rust lines were found under src-tauri/src."
    else
      echo "Changed line coverage: ${line_pct}% (${line_hit:-0}/${line_total:-0})"
      if [[ "${branch_total:-0}" -gt 0 ]]; then
        echo "Changed branch coverage: ${branch_pct}% (${branch_hit:-0}/${branch_total:-0})"
      else
        echo "Changed branch coverage: n/a (no branch points on changed lines)"
      fi
      echo
      echo "## Summary By File"
      awk -F'\t' '
        function pct(hit, total) {
          if (total == 0) {
            return "n/a"
          }
          return sprintf("%.2f", (hit / total) * 100)
        }
        {
          printf "lines %s%% (%s/%s)  branches %s%% (%s/%s)  %s\n",
            pct($2, $3), $2, $3, pct($4, $5), $4, $5, $1
        }
      ' "$file_summary" | sort

      if [[ -s "$missing_lines" ]]; then
        echo
        echo "## Uncovered Changed Lines"
        sort -t$'\t' -k1,1 -k2,2n "$missing_lines" | awk -F'\t' '{
          printf "%s:%s hits=%s\n", $1, $2, $3
        }'
      fi

      if [[ -s "$missing_branches" ]]; then
        echo
        echo "## Uncovered Changed Branches"
        sort -t$'\t' -k1,1 -k2,2n -k3,3n "$missing_branches" | awk -F'\t' '{
          printf "%s:%s branch=%s block=%s hits=%s\n", $1, $2, $3, $4, $5
        }'
      fi
    fi
  } > "$report_path"

  rm -f "$diff_output" "$changed_lines" "$counts_file" "$file_summary" "$missing_lines" "$missing_branches"

  echo "${line_hit:-0} ${line_total:-0} ${branch_hit:-0} ${branch_total:-0}"
}

if [[ ! -f "$COVERAGE_BASELINE_FILE" ]]; then
  report "FAIL: coverage baseline file not found: $COVERAGE_BASELINE_FILE"
  exit 1
fi

# shellcheck disable=SC1090
source "$COVERAGE_BASELINE_FILE"

cd "$REPO_ROOT/src-tauri"
mkdir -p "$REPO_ROOT/coverage"

report "=== Line coverage (stable) ==="
if ! cargo llvm-cov --ignore-filename-regex "$COVERAGE_IGNORE_REGEX" --fail-under-lines 90 --html --output-dir "$REPO_ROOT/coverage/html" 2>&1; then
  report "FAIL: line coverage below 90%"
  exit 1
fi

report ""
report "=== Branch coverage (nightly) ==="
branch_lcov_path="$REPO_ROOT/coverage/branch.lcov"
branch_gap_report_path="$REPO_ROOT/coverage/branch-gaps.txt"
branch_summary_path="$REPO_ROOT/coverage/branch-summary.txt"
changed_code_report_path="$REPO_ROOT/coverage/changed-code-coverage.txt"
branch_gate_failed=0
if ! cargo +nightly llvm-cov --ignore-filename-regex "$COVERAGE_IGNORE_REGEX" --branch --fail-under-lines 90 --fail-under-regions 90 --lcov --output-path "$branch_lcov_path" 2>&1; then
  branch_gate_failed=1
fi

if [[ -f "$branch_lcov_path" ]]; then
  cargo +nightly llvm-cov report --ignore-filename-regex "$COVERAGE_IGNORE_REGEX" --branch --summary-only > "$branch_summary_path"
  # Same nightly profdata re-rendered as a browsable report: per-file drill-down
  # with Line/Region/Branch columns (the stable HTML above lacks branch data).
  cargo +nightly llvm-cov report --ignore-filename-regex "$COVERAGE_IGNORE_REGEX" --branch --html --output-dir "$REPO_ROOT/coverage/html-branch"
  report "Branch HTML report: $REPO_ROOT/coverage/html-branch/html/index.html"
  generate_branch_gap_report "$branch_lcov_path" "$branch_gap_report_path"
  report "Branch gap report: $branch_gap_report_path"

  read -r changed_line_hit changed_line_total changed_branch_hit changed_branch_total < <(
    generate_changed_code_report "$COVERAGE_DIFF_BASE" "$branch_lcov_path" "$changed_code_report_path"
  )
  report "Changed code coverage report: $changed_code_report_path"

  current_lines_pct="$(extract_total_percent "$branch_summary_path" "lines")"
  current_regions_pct="$(extract_total_percent "$branch_summary_path" "regions")"
  current_branches_pct="$(extract_total_percent "$branch_summary_path" "branches")"

  if percent_lt "$current_lines_pct" "$BASELINE_LINES_PCT"; then
    report "FAIL: overall line coverage regressed (${current_lines_pct}% < ${BASELINE_LINES_PCT}%)"
    exit 1
  fi

  if percent_lt "$current_regions_pct" "$BASELINE_REGIONS_PCT"; then
    report "FAIL: overall region coverage regressed (${current_regions_pct}% < ${BASELINE_REGIONS_PCT}%)"
    exit 1
  fi

  if percent_lt "$current_branches_pct" "$BASELINE_BRANCHES_PCT"; then
    report "FAIL: overall branch coverage regressed (${current_branches_pct}% < ${BASELINE_BRANCHES_PCT}%)"
    exit 1
  fi

  changed_lines_pct="$(format_percent "${changed_line_hit:-0}" "${changed_line_total:-0}")"
  if [[ "${changed_line_total:-0}" -gt 0 ]] && percent_lt "$changed_lines_pct" "$CHANGED_CODE_MIN"; then
    report "FAIL: changed Rust line coverage below ${CHANGED_CODE_MIN}% (${changed_lines_pct}%)"
    exit 1
  fi

  changed_branches_pct="$(format_percent "${changed_branch_hit:-0}" "${changed_branch_total:-0}")"
  if [[ "${changed_branch_total:-0}" -gt 0 ]] && percent_lt "$changed_branches_pct" "$CHANGED_CODE_MIN"; then
    report "FAIL: changed Rust branch coverage below ${CHANGED_CODE_MIN}% (${changed_branches_pct}%)"
    exit 1
  fi
fi

if [[ "$branch_gate_failed" -ne 0 ]]; then
  report "FAIL: nightly coverage gate below threshold (lines/regions)"
  exit 1
fi

report ""
report "Coverage checks passed."

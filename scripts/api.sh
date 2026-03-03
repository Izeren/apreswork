#!/usr/bin/env bash
# Copyright 2026 Aleksandr Iushmanov (@izeren)
# SPDX-License-Identifier: Apache-2.0

# Task-focused helper around the Apreswork REST API.
#
# Usage:
#   bash scripts/api.sh list-agenda /tmp/agenda.json START END
#   bash scripts/api.sh list-agenda /tmp/agenda.json START END agent
#   bash scripts/api.sh list-tasks /tmp/tasks.json
#   bash scripts/api.sh list-tasks /tmp/tasks.json 'labels=agent&statuses=scheduled'
#   bash scripts/api.sh list-labels /tmp/labels.json
#   bash scripts/api.sh next-scheduled-agent-task /tmp/next-task.json
#   bash scripts/api.sh next-agent-task /tmp/next-task.json
#   bash scripts/api.sh get-task <task-id> /tmp/task.json
#   bash scripts/api.sh create-task /tmp/new-task.json
#   bash scripts/api.sh create-task /tmp/new-task.json /tmp/created-task.json
#   bash scripts/api.sh update-task /tmp/task.json
#   bash scripts/api.sh update-task /tmp/task.json /tmp/task-updated.json
#   bash scripts/api.sh complete-task /tmp/task.json
#   bash scripts/api.sh delete-task <task-id>
#   bash scripts/api.sh list-comments <task-id> /tmp/comments.json
#   bash scripts/api.sh add-comment <task-id> /tmp/comment-body.json
#   bash scripts/api.sh add-comment <task-id> /tmp/comment-body.json /tmp/created.json
#   bash scripts/api.sh move-chunk <chunk-id> NEW_START NEW_END /tmp/moved.json
#   bash scripts/api.sh whoami /tmp/profile.json
#   bash scripts/api.sh profile-list /tmp/profiles.json
#   bash scripts/api.sh profile-switch <profile-id> /tmp/result.json
#   bash scripts/api.sh profile-switch <profile-id> /tmp/result.json <expected-profile-id>
#   bash scripts/api.sh backup-status /tmp/backup-status.json
#   bash scripts/api.sh backup-now /tmp/backup-status.json

set -euo pipefail

PORT="${APRESWORK_API_PORT:-19532}"
BASE="http://127.0.0.1:${PORT}"

usage() {
  cat >&2 <<'EOF'
Usage:
  bash scripts/api.sh list-agenda OUTPUT_FILE START END [LABEL]
  bash scripts/api.sh list-tasks OUTPUT_FILE [QUERY]
  bash scripts/api.sh list-labels OUTPUT_FILE
  bash scripts/api.sh next-scheduled-agent-task OUTPUT_FILE
  bash scripts/api.sh next-agent-task OUTPUT_FILE
  bash scripts/api.sh get-task TASK_ID OUTPUT_FILE
  bash scripts/api.sh create-task TASK_FILE [OUTPUT_FILE]
  bash scripts/api.sh update-task TASK_FILE [OUTPUT_FILE]
  bash scripts/api.sh complete-task TASK_FILE [OUTPUT_FILE]
  bash scripts/api.sh delete-task TASK_ID
  bash scripts/api.sh list-comments TASK_ID OUTPUT_FILE
  bash scripts/api.sh add-comment TASK_ID BODY_FILE [OUTPUT_FILE]
  bash scripts/api.sh move-chunk CHUNK_ID NEW_START NEW_END OUTPUT_FILE
  bash scripts/api.sh whoami OUTPUT_FILE
  bash scripts/api.sh profile-list OUTPUT_FILE
  bash scripts/api.sh profile-switch PROFILE_ID OUTPUT_FILE [EXPECTED_PROFILE_ID]
  bash scripts/api.sh backup-status OUTPUT_FILE
  bash scripts/api.sh backup-now OUTPUT_FILE
EOF
}

die() {
  echo "api.sh: $*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

ensure_parent_dir() {
  local file_path="$1"
  local parent_dir
  parent_dir="$(dirname "$file_path")"
  mkdir -p "$parent_dir"
}

format_json_file() {
  local input_file="$1"
  local output_file="$2"

  if ! jq '.' "$input_file" >"$output_file"; then
    return 1
  fi
}

# Report a failed request and exit with curl's exit code. The response body
# (if any) goes to stderr; the response file is removed.
report_request_failure() {
  local method="$1"
  local path="$2"
  local response_file="$3"
  local exit_code="$4"

  echo "api.sh: request failed: $method $path" >&2
  if [ -s "$response_file" ]; then
    cat "$response_file" >&2
  else
    echo "api.sh: API unavailable at $BASE" >&2
  fi
  rm -f "$response_file"
  exit "$exit_code"
}

api_request_to_file() {
  local method="$1"
  local path="$2"
  local output_file="$3"
  local input_file="${4:-}"
  local url="${BASE}${path}"
  local response_file
  local curl_status=0

  response_file="$(mktemp)"

  if [ -n "$input_file" ]; then
    [ -f "$input_file" ] || die "input file not found: $input_file"
    curl -sS --fail-with-body -X "$method" "$url" \
      -H "Content-Type: application/json" \
      -H "Accept: application/json" \
      --data-binary "@${input_file}" >"$response_file" || curl_status=$?
  else
    curl -sS --fail-with-body -X "$method" "$url" \
      -H "Accept: application/json" >"$response_file" || curl_status=$?
  fi
  if [ "$curl_status" -ne 0 ]; then
    report_request_failure "$method" "$path" "$response_file" "$curl_status"
  fi

  ensure_parent_dir "$output_file"
  if ! format_json_file "$response_file" "$output_file"; then
    rm -f "$response_file"
    die "invalid JSON response for $method $path"
  fi

  rm -f "$response_file"
}

pick_first_task_to_file() {
  local tasks_file="$1"
  local output_file="$2"
  local selected_file

  selected_file="$(mktemp)"
  if ! jq '
    if type != "array" then
      error("expected array response")
    elif length == 0 then
      empty
    else
      sort_by([
        ({"critical":0,"high":1,"medium":2,"low":3}[(.priority // "") | ascii_downcase] // 4),
        (.deadline // "9999-12-31T23:59:59Z")
      ]) | .[0]
    end
  ' "$tasks_file" >"$selected_file"; then
    rm -f "$selected_file"
    die "unexpected tasks response in $tasks_file"
  fi

  if [ ! -s "$selected_file" ]; then
    rm -f "$selected_file"
    return 1
  fi

  ensure_parent_dir "$output_file"
  if ! format_json_file "$selected_file" "$output_file"; then
    rm -f "$selected_file"
    die "failed to write selected task JSON to $output_file"
  fi
  rm -f "$selected_file"
}

next_agent_task() {
  local output_file="$1"
  local tasks_file

  tasks_file="$(mktemp)"
  api_request_to_file GET "/api/tasks?labels=agent&statuses=scheduled" "$tasks_file"
  if pick_first_task_to_file "$tasks_file" "$output_file"; then
    rm -f "$tasks_file"
    return 0
  fi

  api_request_to_file GET "/api/tasks?labels=agent&statuses=pending" "$tasks_file"
  if pick_first_task_to_file "$tasks_file" "$output_file"; then
    rm -f "$tasks_file"
    return 0
  fi

  rm -f "$tasks_file"
  die "no agent tasks in scheduled or pending status"
}

# "Next" follows the calendar's reading order: the chunk running right now
# (start <= now < end; latest start wins), else the next upcoming chunk
# (earliest future start). A stale past chunk — e.g. a fixed chunk stranded
# before now, which no reschedule ever moves — is never "next"; it surfaces
# via the Status view / past-due requeue instead. Ties break by
# [start_time, priority rank, task_id] so equal-start picks are deterministic.
next_scheduled_agent_task() {
  local output_file="$1"
  local agenda_file
  local now_utc
  local range_start
  local range_end
  local task_id

  agenda_file="$(mktemp)"
  now_utc="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  # Scheduled chunks only exist inside the scheduler horizon; -1d/+30d
  # comfortably covers in-progress chunks and the whole horizon.
  range_start="$(date -u -d '-1 day' +"%Y-%m-%dT%H:%M:%SZ")"
  range_end="$(date -u -d '+30 days' +"%Y-%m-%dT%H:%M:%SZ")"

  api_request_to_file \
    GET \
    "/api/agenda?start=${range_start}&end=${range_end}&label=agent" \
    "$agenda_file"

  task_id="$(jq -r --arg now "$now_utc" '
    def prio_rank:
      {"critical":0,"high":1,"medium":2,"low":3}
        [(.task_priority // "") | ascii_downcase] // 4;
    [ .[] | select(.chunk.status == "scheduled") ] as $items
    | (
        [ $items[]
          | select(.chunk.start_time <= $now and .chunk.end_time > $now)
        ] as $running
        | if ($running | length) > 0 then
            ([ $running[].chunk.start_time ] | max) as $latest
            | [ $running[] | select(.chunk.start_time == $latest) ]
            | sort_by([prio_rank, .chunk.task_id])
            | .[0]
          else
            null
          end
      ) // (
        [ $items[] | select(.chunk.start_time > $now) ]
        | sort_by([.chunk.start_time, prio_rank, .chunk.task_id])
        | .[0]
      ) // empty
    | .chunk.task_id // empty
  ' "$agenda_file")"

  rm -f "$agenda_file"

  if [ -n "$task_id" ]; then
    get_task "$task_id" "$output_file"
    return 0
  fi

  next_agent_task "$output_file"
}

list_tasks() {
  local output_file="$1"
  local query="${2:-}"
  local path="/api/tasks"

  if [ -n "$query" ]; then
    path="${path}?${query#\?}"
  fi

  api_request_to_file GET "$path" "$output_file"
}

list_labels() {
  local output_file="$1"

  api_request_to_file GET "/api/labels" "$output_file"
}

# The REST server serves exactly one (unlocked) profile; whoami reports whose
# data the API is currently touching.
whoami_profile() {
  local output_file="$1"

  api_request_to_file GET "/api/profile" "$output_file"
}

backup_status() {
  local output_file="$1"

  api_request_to_file GET "/api/backup/status" "$output_file"
}

list_profiles() {
  local output_file="$1"

  api_request_to_file GET "/api/profiles" "$output_file"
}

# EXPECTED_PROFILE_ID is optional; when provided the server returns 409 if the
# active profile differs — guards against concurrent switches.
switch_profile() {
  local profile_id="$1"
  local output_file="$2"
  local expected_profile_id="${3:-}"
  local body_file

  [ -n "$profile_id" ] || die "profile id is required"
  [ -n "$output_file" ] || die "output file is required"

  body_file="$(mktemp)"
  if [ -n "$expected_profile_id" ]; then
    jq -n --arg pid "$profile_id" --arg eid "$expected_profile_id" \
      '{profile_id: $pid, expected_profile_id: $eid}' >"$body_file"
  else
    jq -n --arg pid "$profile_id" \
      '{profile_id: $pid}' >"$body_file"
  fi

  api_request_to_file POST "/api/profile/switch" "$output_file" "$body_file"
  rm -f "$body_file"
}

# Manual export to the configured backup target; writes the fresh status.
backup_now() {
  local output_file="$1"

  api_request_to_file POST "/api/backup/now" "$output_file"
}

list_agenda() {
  local output_file="$1"
  local start="$2"
  local end="$3"
  local label="${4:-}"
  local path="/api/agenda?start=${start}&end=${end}"

  if [ -n "$label" ]; then
    path="${path}&label=${label}"
  fi

  api_request_to_file GET "$path" "$output_file"
}

get_task() {
  local task_id="$1"
  local output_file="$2"

  [ -n "$task_id" ] || die "task id is required"
  api_request_to_file GET "/api/tasks/${task_id}" "$output_file"
}

create_task() {
  local task_file="$1"
  local output_file="${2:-$1}"

  [ -f "$task_file" ] || die "task file not found: $task_file"

  if ! jq -e '
    type == "object"
    and (.title | type == "string" and length > 0)
    and (.description | type == "string")
    and (.duration_minutes | type == "number")
    and (.priority | type == "string" and length > 0)
    and ((.start_date == null) or (.start_date | type == "string"))
    and ((.deadline == null) or (.deadline | type == "string"))
    and ((.schedule_id == null) or (.schedule_id | type == "string"))
    and (.min_chunk_minutes | type == "number")
    and (.no_split | type == "boolean")
    and (.labels | type == "array")
    and ((.status == null) or (.status | type == "string" and length > 0))
  ' "$task_file" >/dev/null; then
    die "task file is missing required create-task fields: $task_file"
  fi

  api_request_to_file POST "/api/tasks" "$output_file" "$task_file"
}

build_task_patch_file() {
  local task_id="$1"
  local task_file="$2"
  local patch_file="$3"
  local current_task_file

  [ -f "$task_file" ] || die "task file not found: $task_file"
  current_task_file="$(mktemp)"

  api_request_to_file GET "/api/tasks/${task_id}" "$current_task_file"

  if ! jq -s '
    .[0] as $current
    | (.[0] * .[1]) as $merged
    | reduce [
        "title",
        "description",
        "duration_minutes",
        "priority",
        "start_date",
        "deadline",
        "schedule_id",
        "min_chunk_minutes",
        "no_split",
        "labels",
        "status"
      ][] as $key (
        {};
        if $current[$key] != $merged[$key] then
          . + { ($key): $merged[$key] }
        else
          .
        end
      )
  ' "$current_task_file" "$task_file" >"$patch_file"; then
    rm -f "$current_task_file"
    die "invalid task JSON in $task_file"
  fi

  rm -f "$current_task_file"
}

update_task() {
  local task_file="$1"
  local output_file="${2:-$1}"
  local task_id
  local patch_file

  [ -f "$task_file" ] || die "task file not found: $task_file"

  task_id="$(jq -r '.id // empty' "$task_file")"
  [ -n "$task_id" ] || die "task file is missing .id: $task_file"

  patch_file="$(mktemp)"
  build_task_patch_file "$task_id" "$task_file" "$patch_file"
  api_request_to_file PATCH "/api/tasks/${task_id}" "$output_file" "$patch_file"
  rm -f "$patch_file"
}

complete_task() {
  local task_file="$1"
  local output_file="${2:-$1}"
  local task_id

  [ -f "$task_file" ] || die "task file not found: $task_file"

  task_id="$(jq -r '.id // empty' "$task_file")"
  [ -n "$task_id" ] || die "task file is missing .id: $task_file"

  api_request_to_file POST "/api/tasks/${task_id}/complete" "$output_file"
}

# DELETE returns 204 No Content on success (no JSON body to write), so this
# bypasses api_request_to_file. A recurring instance is cancelled server-side
# instead of deleted so its cadence slot stays occupied.
delete_task() {
  local task_id="$1"
  local response_file
  local curl_status=0

  [ -n "$task_id" ] || die "task id is required"

  response_file="$(mktemp)"
  curl -sS --fail-with-body -X DELETE "${BASE}/api/tasks/${task_id}" \
    -H "Accept: application/json" >"$response_file" || curl_status=$?
  if [ "$curl_status" -ne 0 ]; then
    report_request_failure DELETE "/api/tasks/${task_id}" "$response_file" "$curl_status"
  fi

  rm -f "$response_file"
  echo "deleted task ${task_id}"
}

list_comments() {
  local task_id="$1"
  local output_file="$2"

  [ -n "$task_id" ] || die "task id is required"
  api_request_to_file GET "/api/tasks/${task_id}/comments" "$output_file"
}

# BODY_FILE is JSON: {"content": "..."} with an optional "author" (defaults
# to "User" server-side; "SYSTEM" is reserved and rejected).
add_comment() {
  local task_id="$1"
  local body_file="$2"
  local output_file="${3:-$2}"

  [ -n "$task_id" ] || die "task id is required"
  [ -f "$body_file" ] || die "comment body file not found: $body_file"

  if ! jq -e '
    type == "object"
    and (.content | type == "string" and length > 0)
    and ((.author == null) or (.author | type == "string"))
  ' "$body_file" >/dev/null; then
    die "comment body must be an object with non-empty .content: $body_file"
  fi

  api_request_to_file POST "/api/tasks/${task_id}/comments" "$output_file" "$body_file"
}

move_chunk() {
  local chunk_id="$1"
  local new_start="$2"
  local new_end="$3"
  local output_file="$4"
  local body_file

  [ -n "$chunk_id" ] || die "chunk id is required"
  [ -n "$new_start" ] || die "new_start is required"
  [ -n "$new_end" ] || die "new_end is required"
  [ -n "$output_file" ] || die "output file is required"

  body_file="$(mktemp)"
  if ! jq -n --arg start "$new_start" --arg end "$new_end" \
    '{new_start: $start, new_end: $end}' >"$body_file"; then
    rm -f "$body_file"
    die "failed to build move-chunk request body"
  fi

  api_request_to_file POST "/api/chunks/${chunk_id}/move" "$output_file" "$body_file"
  rm -f "$body_file"
}

if [ "$#" -lt 2 ]; then
  usage
  exit 1
fi

require_command curl
require_command jq

COMMAND="$1"
shift

case "$COMMAND" in
  list-agenda)
    if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
      usage
      exit 1
    fi
    list_agenda "$@"
    ;;
  list-tasks)
    if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
      usage
      exit 1
    fi
    list_tasks "$@"
    ;;
  list-labels)
    if [ "$#" -ne 1 ]; then
      usage
      exit 1
    fi
    list_labels "$1"
    ;;
  next-scheduled-agent-task)
    if [ "$#" -ne 1 ]; then
      usage
      exit 1
    fi
    next_scheduled_agent_task "$1"
    ;;
  next-agent-task)
    if [ "$#" -ne 1 ]; then
      usage
      exit 1
    fi
    next_agent_task "$1"
    ;;
  get-task)
    if [ "$#" -ne 2 ]; then
      usage
      exit 1
    fi
    get_task "$1" "$2"
    ;;
  create-task)
    if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
      usage
      exit 1
    fi
    create_task "$@"
    ;;
  update-task)
    if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
      usage
      exit 1
    fi
    update_task "$@"
    ;;
  complete-task)
    if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
      usage
      exit 1
    fi
    complete_task "$@"
    ;;
  delete-task)
    if [ "$#" -ne 1 ]; then
      usage
      exit 1
    fi
    delete_task "$1"
    ;;
  list-comments)
    if [ "$#" -ne 2 ]; then
      usage
      exit 1
    fi
    list_comments "$1" "$2"
    ;;
  add-comment)
    if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
      usage
      exit 1
    fi
    add_comment "$@"
    ;;
  move-chunk)
    if [ "$#" -ne 4 ]; then
      usage
      exit 1
    fi
    move_chunk "$@"
    ;;
  whoami)
    if [ "$#" -ne 1 ]; then
      usage
      exit 1
    fi
    whoami_profile "$1"
    ;;
  profile-list)
    if [ "$#" -ne 1 ]; then
      usage
      exit 1
    fi
    list_profiles "$1"
    ;;
  profile-switch)
    if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
      usage
      exit 1
    fi
    switch_profile "$@"
    ;;
  backup-status)
    if [ "$#" -ne 1 ]; then
      usage
      exit 1
    fi
    backup_status "$1"
    ;;
  backup-now)
    if [ "$#" -ne 1 ]; then
      usage
      exit 1
    fi
    backup_now "$1"
    ;;
  *)
    usage
    exit 1
    ;;
esac

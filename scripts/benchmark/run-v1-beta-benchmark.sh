#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/benchmark/run-v1-beta-benchmark.sh --target-repo <path> [--incremental-from <ref>] [--output-json <path>] [--output-markdown <path>]

Options:
  --target-repo        Target repository path (required)
  --incremental-from   Git ref for incremental scenario (default: HEAD~1)
  --output-json        Output JSON artifact path
  --output-markdown    Output Markdown artifact path
USAGE
}

TARGET_REPO=""
INCREMENTAL_FROM="HEAD~1"
OUTPUT_JSON=""
OUTPUT_MARKDOWN=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target-repo)
      TARGET_REPO="${2:-}"
      shift 2
      ;;
    --incremental-from)
      INCREMENTAL_FROM="${2:-}"
      shift 2
      ;;
    --output-json)
      OUTPUT_JSON="${2:-}"
      shift 2
      ;;
    --output-markdown)
      OUTPUT_MARKDOWN="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ -z "$TARGET_REPO" ]]; then
  echo "error: --target-repo is required" >&2
  usage
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required for benchmark harness" >&2
  exit 1
fi

if ! command -v git >/dev/null 2>&1; then
  echo "error: git is required for benchmark harness" >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo is required for benchmark harness" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd -P)"
TARGET_REPO="$(cd "$TARGET_REPO" && pwd -P)"
CONFIG_PATH="$TARGET_REPO/.reviva/config.toml"

if [[ ! -f "$CONFIG_PATH" ]]; then
  echo "warning: missing config: $CONFIG_PATH (continuing with reviva default config values)" >&2
fi

if [[ -z "$OUTPUT_JSON" ]]; then
  OUTPUT_JSON="$WORKSPACE_ROOT/docs/artifacts/v1-beta-benchmark.json"
fi
if [[ -z "$OUTPUT_MARKDOWN" ]]; then
  OUTPUT_MARKDOWN="$WORKSPACE_ROOT/docs/artifacts/v1-beta-benchmark.md"
fi

mkdir -p "$(dirname "$OUTPUT_JSON")"
mkdir -p "$(dirname "$OUTPUT_MARKDOWN")"

now_ms() {
  if command -v python3 >/dev/null 2>&1; then
    python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
    return
  fi
  if command -v perl >/dev/null 2>&1; then
    perl -MTime::HiRes=time -e 'printf("%.0f\n", time() * 1000);'
    return
  fi
  echo "$(( $(date +%s) * 1000 ))"
}

now_sec() {
  date +%s
}

get_warning_value() {
  local session_file="$1"
  local prefix="$2"
  jq -r --arg p "$prefix" '.warnings[]? | select(startswith($p)) | ltrimstr($p)' "$session_file" | head -n1
}

tmp_git_config="$(mktemp "$WORKSPACE_ROOT/.tmp-benchmark-gitconfig.XXXXXX")"
cleanup() {
  rm -f "$tmp_git_config"
}
trap cleanup EXIT

git config --file "$tmp_git_config" --add safe.directory "$TARGET_REPO"

(
  cd "$WORKSPACE_ROOT"
  cargo build --quiet -p reviva-cli >/dev/null
)

REVIVA_BIN="$WORKSPACE_ROOT/target/debug/reviva"
if [[ ! -x "$REVIVA_BIN" ]]; then
  echo "error: reviva binary not found after build: $REVIVA_BIN" >&2
  exit 1
fi

scan_output="$("$REVIVA_BIN" scan --repo "$TARGET_REPO" 2>/dev/null)"
if [[ -z "$scan_output" ]]; then
  echo "error: reviva scan returned no reviewable files for benchmark" >&2
  exit 1
fi
mapfile -t scan_paths < <(
  printf '%s\n' "$scan_output" \
    | awk '{
        path=$1
        tokens=0
        for(i=1;i<=NF;i++){
          if($i ~ /^estimated_tokens=/){
            split($i,a,"=")
            tokens=a[2]
          }
        }
        if(path!=""){print tokens "\t" path}
      }' \
    | sort -n -k1,1 -k2,2 \
    | awk -F '\t' '{print $2}' \
    | sed '/^\s*$/d'
)
if [[ ${#scan_paths[@]} -eq 0 ]]; then
  echo "error: reviva scan returned no reviewable files for benchmark" >&2
  exit 1
fi
full_miss_file="${scan_paths[0]}"
full_set_count=5
if [[ ${#scan_paths[@]} -lt $full_set_count ]]; then
  full_set_count=${#scan_paths[@]}
fi

RUN_ID="$(now_ms)"

FULL_MISS_ARGS=(
  --repo "$TARGET_REPO"
  --mode launch-readiness
  --profile launch-readiness
  --max-findings 4
  --max-output-tokens 512
  --note "benchmark_run=$RUN_ID scenario=full"
  --file "$full_miss_file"
)
FULL_SET_MISS_ARGS=(
  --repo "$TARGET_REPO"
  --mode launch-readiness
  --profile launch-readiness
  --max-findings 4
  --max-output-tokens 512
  --note "benchmark_run=$RUN_ID scenario=full_set"
)
for ((i = 0; i < full_set_count; i++)); do
  FULL_SET_MISS_ARGS+=(--file "${scan_paths[$i]}")
done
INCREMENTAL_MISS_ARGS=(
  --repo "$TARGET_REPO"
  --mode launch-readiness
  --profile launch-readiness
  --max-findings 4
  --max-output-tokens 512
  --note "benchmark_run=$RUN_ID scenario=incremental"
  --incremental-from "$INCREMENTAL_FROM"
)

run_reviva_review() {
  local scenario="$1"
  local session_id="$2"
  shift 2
  local -a args=("$@")
  local timestamp start_ms end_ms elapsed_ms session_file findings_count
  local review_cache review_cache_source incremental_scope incremental_file_count incremental_fallback_full_file_count

  timestamp="$(now_sec)"
  start_ms="$(now_ms)"
  (
    cd "$TARGET_REPO"
    REVIVA_TEST_SESSION_ID="$session_id" \
    REVIVA_TEST_TIMESTAMP="$timestamp" \
    GIT_CONFIG_GLOBAL="$tmp_git_config" \
    "$REVIVA_BIN" review "${args[@]}" >/dev/null
  )
  end_ms="$(now_ms)"
  elapsed_ms=$(( end_ms - start_ms ))

  session_file="$TARGET_REPO/.reviva/sessions/${session_id}.json"
  if [[ ! -f "$session_file" ]]; then
    echo "error: session artifact not found: $session_file" >&2
    exit 1
  fi

  findings_count="$(jq '.findings | length' "$session_file")"
  review_cache="$(get_warning_value "$session_file" "review_cache=")"
  review_cache_source="$(get_warning_value "$session_file" "review_cache_source=")"
  incremental_scope="$(get_warning_value "$session_file" "incremental_scope=")"
  incremental_file_count="$(get_warning_value "$session_file" "incremental_file_count=")"
  incremental_fallback_full_file_count="$(get_warning_value "$session_file" "incremental_fallback_full_file_count=")"

  jq -n \
    --arg scenario "$scenario" \
    --arg session_id "$session_id" \
    --argjson elapsed_ms "$elapsed_ms" \
    --argjson findings_count "$findings_count" \
    --arg review_cache "${review_cache:-}" \
    --arg review_cache_source "${review_cache_source:-}" \
    --arg incremental_scope "${incremental_scope:-}" \
    --arg incremental_file_count "${incremental_file_count:-}" \
    --arg incremental_fallback_full_file_count "${incremental_fallback_full_file_count:-}" \
    '{
      scenario: $scenario,
      session_id: $session_id,
      elapsed_ms: $elapsed_ms,
      findings_count: $findings_count,
      review_cache: (if $review_cache == "" then null else $review_cache end),
      review_cache_source: (if $review_cache_source == "" then null else $review_cache_source end),
      incremental_scope: (if $incremental_scope == "" then null else $incremental_scope end),
      incremental_file_count: (if $incremental_file_count == "" then null else $incremental_file_count end),
      incremental_fallback_full_file_count: (if $incremental_fallback_full_file_count == "" then null else $incremental_fallback_full_file_count end)
    }'
}

scenario_full_miss="$(run_reviva_review full_miss m6p2-full-miss-1 "${FULL_MISS_ARGS[@]}")"
scenario_full_hit="$(run_reviva_review full_hit m6p2-full-hit-1 "${FULL_MISS_ARGS[@]}")"
scenario_full_set_miss="$(run_reviva_review full_set_miss m6p2-fullset-miss-1 "${FULL_SET_MISS_ARGS[@]}")"
scenario_incremental_miss="$(run_reviva_review incremental_miss m6p2-incremental-miss-1 "${INCREMENTAL_MISS_ARGS[@]}")"

full_miss_cache="$(jq -r '.review_cache // ""' <<<"$scenario_full_miss")"
full_hit_cache="$(jq -r '.review_cache // ""' <<<"$scenario_full_hit")"
full_set_miss_cache="$(jq -r '.review_cache // ""' <<<"$scenario_full_set_miss")"
incremental_miss_cache="$(jq -r '.review_cache // ""' <<<"$scenario_incremental_miss")"

if [[ "$full_miss_cache" != "miss" ]]; then
  echo "error: benchmark expectation failed: full_miss must be review_cache=miss (got '$full_miss_cache')" >&2
  exit 1
fi
if [[ "$full_hit_cache" != "hit" ]]; then
  echo "error: benchmark expectation failed: full_hit must be review_cache=hit (got '$full_hit_cache')" >&2
  exit 1
fi
if [[ "$full_set_miss_cache" != "miss" ]]; then
  echo "error: benchmark expectation failed: full_set_miss must be review_cache=miss (got '$full_set_miss_cache')" >&2
  exit 1
fi
if [[ "$incremental_miss_cache" != "miss" ]]; then
  echo "error: benchmark expectation failed: incremental_miss must be review_cache=miss (got '$incremental_miss_cache')" >&2
  exit 1
fi

full_miss_elapsed="$(jq '.elapsed_ms' <<<"$scenario_full_miss")"
full_hit_elapsed="$(jq '.elapsed_ms' <<<"$scenario_full_hit")"
full_set_miss_elapsed="$(jq '.elapsed_ms' <<<"$scenario_full_set_miss")"
incremental_miss_elapsed="$(jq '.elapsed_ms' <<<"$scenario_incremental_miss")"

cache_gain_percent="$(jq -n --argjson miss "$full_miss_elapsed" --argjson hit "$full_hit_elapsed" 'if $miss > 0 then (((1 - ($hit / $miss)) * 10000) | round / 100) else null end')"
incremental_gain_percent="$(jq -n --argjson full "$full_set_miss_elapsed" --argjson inc "$incremental_miss_elapsed" 'if $full > 0 then (((1 - ($inc / $full)) * 10000) | round / 100) else null end')"

scenarios_json="$(printf '%s\n' "$scenario_full_miss" "$scenario_full_hit" "$scenario_full_set_miss" "$scenario_incremental_miss" | jq -s '.')"
generated_at_utc="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

jq -n \
  --arg generated_at_utc "$generated_at_utc" \
  --arg target_repo "$TARGET_REPO" \
  --arg incremental_from "$INCREMENTAL_FROM" \
  --argjson scenarios "$scenarios_json" \
  --argjson cache_gain_percent "$cache_gain_percent" \
  --argjson incremental_gain_percent "$incremental_gain_percent" \
  '{
    generated_at_utc: $generated_at_utc,
    target_repo: $target_repo,
    incremental_from: $incremental_from,
    scenarios: $scenarios,
    derived_metrics: {
      cache_gain_percent: $cache_gain_percent,
      incremental_gain_percent: $incremental_gain_percent
    }
  }' > "$OUTPUT_JSON"

markdown_row() {
  local scenario_json="$1"
  local scenario session_id elapsed_ms cache cache_source inc_scope inc_files inc_fallback findings
  scenario="$(jq -r '.scenario' <<<"$scenario_json")"
  session_id="$(jq -r '.session_id' <<<"$scenario_json")"
  elapsed_ms="$(jq -r '.elapsed_ms' <<<"$scenario_json")"
  cache="$(jq -r '.review_cache // ""' <<<"$scenario_json")"
  cache_source="$(jq -r '.review_cache_source // ""' <<<"$scenario_json")"
  inc_scope="$(jq -r '.incremental_scope // ""' <<<"$scenario_json")"
  inc_files="$(jq -r '.incremental_file_count // ""' <<<"$scenario_json")"
  inc_fallback="$(jq -r '.incremental_fallback_full_file_count // ""' <<<"$scenario_json")"
  findings="$(jq -r '.findings_count' <<<"$scenario_json")"
  printf '| %s | %s | %s | %s | %s | %s | %s | %s | %s |\n' \
    "$scenario" "$session_id" "$elapsed_ms" "$cache" "$cache_source" "$inc_scope" "$inc_files" "$inc_fallback" "$findings"
}

{
  echo "# Reviva v1-beta Benchmark Artifact"
  echo
  echo "- Generated At (UTC): $generated_at_utc"
  echo "- Target Repo: $TARGET_REPO"
  echo "- Incremental From: $INCREMENTAL_FROM"
  echo
  echo "## Scenario Results"
  echo
  echo "| Scenario | Session ID | Elapsed (ms) | Cache | Cache Source | Incremental Scope | Incremental Files | Fallback Full Files | Findings |"
  echo "| --- | --- | ---: | --- | --- | --- | ---: | ---: | ---: |"
  markdown_row "$scenario_full_miss"
  markdown_row "$scenario_full_hit"
  markdown_row "$scenario_full_set_miss"
  markdown_row "$scenario_incremental_miss"
  echo
  echo "## Derived Metrics"
  echo
  echo "- Cache gain percent: $cache_gain_percent"
  echo "- Incremental gain percent: $incremental_gain_percent"
  echo
  echo "## Scope Note"
  echo
  echo "- incremental_scope=diff_hunks means only git diff hunks are sent."
  echo "- incremental_fallback_full_file_count>0 means those files were reviewed with full file content."
} > "$OUTPUT_MARKDOWN"

echo "benchmark json: $OUTPUT_JSON"
echo "benchmark markdown: $OUTPUT_MARKDOWN"

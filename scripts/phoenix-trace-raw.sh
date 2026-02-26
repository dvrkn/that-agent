#!/bin/sh
set -eu

usage() {
  cat <<'EOF'
Usage:
  scripts/phoenix-trace-raw.sh <trace-id-or-eval-run-id> [output-file]

Description:
  Fetches a raw trace payload from Phoenix using the official Phoenix CLI.
  The first argument can be either:
  - an OTel trace id, or
  - an eval run id (it will read ~/.that-agent/evals/<run-id>/report.json and extract trace_id).

Environment:
  PHOENIX_HOST      Optional. Default: http://localhost:6006
  PHOENIX_PROJECT   Optional. If set, passed to CLI as --project.
  PHOENIX_API_KEY   Optional. If set, passed to CLI as --api-key.
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ] || [ "$#" -lt 1 ]; then
  usage
  exit 1
fi

ref="$1"
out="${2:-}"

trace_id="$ref"
report_path="$HOME/.that-agent/evals/$ref/report.json"

if [ -f "$report_path" ]; then
  trace_id="$(python3 -c 'import json,sys; p=sys.argv[1]; d=json.load(open(p,"r",encoding="utf-8")); print(d.get("trace_id") or "")' "$report_path" 2>/dev/null || true)"
fi

if [ -z "$trace_id" ]; then
  echo "error: could not resolve trace_id from '$ref'" >&2
  exit 2
fi

if ! command -v npx >/dev/null 2>&1; then
  echo "error: npx not found. Install Node.js/npm (or run: npm i -g @arizeai/phoenix-cli)." >&2
  exit 3
fi

host="${PHOENIX_HOST:-http://localhost:6006}"

set -- npx -y @arizeai/phoenix-cli trace "$trace_id" --format raw --no-progress
if [ -n "${PHOENIX_PROJECT:-}" ]; then
  set -- "$@" --project "$PHOENIX_PROJECT"
fi
if [ -n "${PHOENIX_API_KEY:-}" ]; then
  set -- "$@" --api-key "$PHOENIX_API_KEY"
fi

if [ -n "$out" ]; then
  PHOENIX_HOST="$host" "$@" >"$out"
else
  PHOENIX_HOST="$host" "$@"
fi

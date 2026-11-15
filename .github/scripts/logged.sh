#!/usr/bin/env bash
# Run a command, mirror its output to the job log and keep a copy on disk so
# `publish-logs.sh` can publish it at the end of the job.
#
# Usage: .github/scripts/logged.sh <step-name> <command> [args...]
set -uo pipefail

if [ "$#" -lt 2 ]; then
    echo "usage: $0 <step-name> <command> [args...]" >&2
    exit 2
fi

name="$1"
shift
log_dir="${CI_LOG_DIR:-${RUNNER_TEMP:-/tmp}/ci-logs}"
# On Windows runners RUNNER_TEMP uses backslashes; bash needs forward slashes.
log_dir="${log_dir//\\//}"
mkdir -p "$log_dir"
log="$log_dir/$name.log"

echo "\$ $*" >"$log"
"$@" 2>&1 | tee -a "$log"
status="${PIPESTATUS[0]}"
echo "[exit status: $status]" >>"$log"
# Make compiler diagnostics visible in the GitHub Checks API as well as the
# uploaded log. This remains useful when artifact storage is unavailable.
if [ "$status" -ne 0 ] && [ "${GITHUB_ACTIONS:-}" = "true" ]; then
    awk '/^error(\[|:)/ { remaining=8 } remaining > 0 { print; remaining-- }' "$log" |
        head -80 |
        while IFS= read -r line; do
            line="${line//'%'/'%25'}"
            line="${line//$'\r'/'%0D'}"
            printf '::error::%s\n' "$line"
        done
fi
exit "$status"

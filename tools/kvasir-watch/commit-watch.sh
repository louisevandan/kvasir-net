#!/usr/bin/env bash
# Retired: this job used to poll the watched repositories every twenty minutes
# and post new commits to the group. It no longer sends anything.
#
# Kept as a stub rather than deleted, because `com.kvasir.watch.commits.plist`
# still exists here and in ~/Library/LaunchAgents/disabled-kvasir-watch/. If
# either is ever loaded again, this is what it will run — a no-op — instead of
# a poller that quietly resumes posting.
#
# `commits.mjs` is untouched and still works by hand:
#
#     node commits.mjs          # prints what is new; sends nothing
#
# It exits 10 when it has something to show, which is what the old job keyed on.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
LOG_DIR="${KVASIR_WATCH_LOG:-$HERE/log}"
mkdir -p "$LOG_DIR"
echo "$(date -Iseconds) commit-watch is retired; nothing sent" >>"$LOG_DIR/commits.log"
exit 0

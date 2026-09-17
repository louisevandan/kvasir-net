#!/bin/zsh
# Publish the two readiness artifacts from their local files.
#
# The Artifact tool lives inside a Claude session, so the publish runs through
# `claude -p` with a prompt narrow enough to be boring: read the artifact that
# already exists at this URL, then publish this file to it. No editing, no
# judgement — the file was already written by readiness.mjs.
set -u
HERE="${0:A:h}"
LOG_DIR="${KVASIR_WATCH_LOG:-$HERE/log}"
mkdir -p "$LOG_DIR"
CONFIG="${KVASIR_WATCH_CONFIG:-$HERE/config.json}"

for lang in en ko; do
  file="$HERE/$(python3 -c "import json;print(json.load(open('$CONFIG'))['readiness']['$lang']['file'])")"
  url=$(python3 -c "import json;print(json.load(open('$CONFIG'))['readiness']['$lang']['artifact'])")
  [[ -f "$file" ]] || { echo "missing $file" >>"$LOG_DIR/readiness.log"; continue; }

  claude -p --permission-mode bypassPermissions \
    "Update one existing artifact, nothing else.

     1. Read the artifact at $url (Artifact tool, action: read).
     2. Publish the local file $file to that same artifact URL, byte for byte —
        pass url: \"$url\" so it updates in place and keeps its link.

     Do not edit the file's content, do not change its title or favicon, and do
     not create a new artifact. Reply with one line: the URL and whether the
     publish succeeded." >>"$LOG_DIR/readiness.log" 2>&1 \
    && echo "$(date -Iseconds) published $lang" >>"$LOG_DIR/readiness.log" \
    || echo "$(date -Iseconds) publish failed for $lang" >>"$LOG_DIR/readiness.log"
done

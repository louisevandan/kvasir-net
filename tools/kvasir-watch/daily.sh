#!/usr/bin/env bash
# One day's run: collect, render, send. Keeps every artifact it produced, so a
# report that looked wrong can be read back against the facts it came from.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
LOG_DIR="${KVASIR_WATCH_LOG:-$HERE/log}"
DAY="$(date +%Y-%m-%d)"
mkdir -p "$LOG_DIR"
exec >>"$LOG_DIR/$DAY.log" 2>&1
echo "=== $(date -Iseconds) start ==="

# Credentials live outside the repo.
ENV_FILE="${KVASIR_WATCH_ENV:-$HOME/project/any/.env}"
if [[ -f "$ENV_FILE" ]]; then
  set -a; source "$ENV_FILE"; set +a
else
  echo "no env file at $ENV_FILE — cannot send"; exit 2
fi

# Refresh the mirror before collecting. Nothing else updates it, so without this
# the report describes the repository as it stood the day the clone was made —
# which is exactly what had been happening: a mirror stuck at one commit reads as
# "0 commits today" every day, and that is indistinguishable from a quiet day.
#
# Fast-forward only, deliberately. A ff-only merge cannot discard a commit and
# refuses outright on a dirty tree, so pointing this at a working checkout by
# mistake costs a skipped update, not someone's afternoon. The flag is a second
# guard: the unit beside the fleet sets it, a laptop running this by hand does
# not. A failed refresh is not fatal — slightly old facts beat no report.
if [[ "${KVASIR_WATCH_MIRROR:-0}" == "1" ]]; then
  REPO="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["repo"])' "$HERE/config.json" 2>/dev/null || true)"
  if [[ -n "${REPO:-}" && -d "$REPO/.git" ]]; then
    BRANCH="$(git -C "$REPO" branch --show-current)"
    if git -C "$REPO" fetch --quiet --prune origin "$BRANCH" \
       && git -C "$REPO" merge --quiet --ff-only "origin/$BRANCH"; then
      echo "$(date -Iseconds) mirror $BRANCH at $(git -C "$REPO" rev-parse --short HEAD)"
    else
      echo "$(date -Iseconds) mirror not fast-forwarded; collecting from what is on disk"
    fi
  fi
fi

JSON="$LOG_DIR/$DAY.json"
HTML="$LOG_DIR/kvasir-$DAY.html"
SUMMARY="$LOG_DIR/$DAY.txt"
SITE_DIR="${KVASIR_SITE_DIR:-$HERE/../../kvasir-home}"

if ! node "$HERE/collect.mjs" >"$JSON"; then
  # A failed collection is still news: say so in the group rather than go quiet.
  printf 'Kvasir · %s\n\nThe daily collection failed. See the runner log.\n' "$DAY" >"$SUMMARY"
  TELEGRAM_BOT_TOKEN="$TELEGRAM_BOT_TOKEN" TELEGRAM_CHAT_ID="$TELEGRAM_CHAT_ID" \
    node "$HERE/send.mjs" "$SUMMARY"
  echo "=== $(date -Iseconds) collect failed ==="
  exit 1
fi

node "$HERE/report.mjs" "$JSON" "$HTML" >"$SUMMARY" || exit 1

# The readiness review is the document people actually read, so the day's facts
# go into it — both languages, so a Korean and an English reader never see
# different numbers — and the English copy is what the group receives.
#
# Not the claude.ai artifact link: a headless `claude -p` authenticates into a
# different artifact space and cannot update those pages, so a link posted here
# every morning would point at whatever was last published by hand. The file
# opens anywhere, needs no account, and is always the current one.
REVIEW="$HERE/readiness/en.html"
# What the group settled overnight, from its own transcript. Read-only: this
# produces a list, it does not change a status anywhere.
CHAT_OUT="$LOG_DIR/chat-$DAY.txt"
if [ -f "$HERE/chat-tasks.mjs" ]; then
  node "$HERE/chat-tasks.mjs" 24 >"$CHAT_OUT" 2>>"$LOG_DIR/chat.log"
  if [ $? -eq 10 ]; then
    printf '\n' >>"$SUMMARY"
    cat "$CHAT_OUT" >>"$SUMMARY"
  fi
fi

# The seed pipeline is money with dates on it, and the dates are the part that
# cannot be caught up on. Only deadlines inside a fortnight and statuses that
# actually moved are reported; a daily line about a table nobody touched is how
# a report stops being read.
SEED_OUT="$LOG_DIR/seed-$DAY.txt"
if [ -f "$HERE/seed.mjs" ]; then
  node "$HERE/seed.mjs" --brief >"$SEED_OUT" 2>>"$LOG_DIR/seed.log"
  if [ $? -eq 10 ]; then
    printf '\n' >>"$SUMMARY"
    cat "$SEED_OUT" >>"$SUMMARY"
  fi
fi

# The Ring is announced only when it changed. The daily line above already
# says what is up; this is the line that says what moved, and it stays silent
# on a quiet day so that the day it speaks, it is read.
RING_OUT="$LOG_DIR/ring-$DAY.txt"
if [ -f "$HERE/ring-alert.mjs" ]; then
  node "$HERE/ring-alert.mjs" "$JSON" >"$RING_OUT" 2>>"$LOG_DIR/ring.log"
  if [ $? -eq 10 ]; then
    printf '\n' >>"$SUMMARY"
    cat "$RING_OUT" >>"$SUMMARY"
  fi
fi

# The judgement at the end of the review is written fresh from the same facts;
# if that run fails the previous one stays, clearly dated, rather than a gap.
node "$HERE/assess.mjs" "$JSON" >>"$LOG_DIR/readiness.log" 2>&1 \
  || echo "$(date -Iseconds) assessment kept from the previous run" >>"$LOG_DIR/readiness.log"

if node "$HERE/readiness.mjs" "$JSON" >>"$LOG_DIR/readiness.log" 2>&1; then
  ATTACH="$LOG_DIR/kvasir-readiness-$DAY.html"
  cp "$REVIEW" "$ATTACH"
else
  printf '\nThe readiness review could not be updated today; the plain report is attached instead.\n' >>"$SUMMARY"
  ATTACH="$HTML"
fi

node "$HERE/send.mjs" "$SUMMARY" "$ATTACH" || exit 1

# A shipped version belongs on the site, not only in a chat message. The check
# writes the entry; publishing it is a separate, explicit step so an automated
# job never pushes a page nobody looked at unless that was asked for.
RELEASE_OUT="$LOG_DIR/release-$DAY.txt"
node "$HERE/release-check.mjs" >"$RELEASE_OUT"
if [[ $? -eq 10 ]]; then
  node "$HERE/send.mjs" "$RELEASE_OUT"
  if [[ "${KVASIR_RELEASE_AUTODEPLOY:-0}" == "1" ]]; then
    ( cd "$SITE_DIR" && npm run build && npx wrangler pages deploy dist --project-name kvasir-home --branch main ) \
      && echo "$(date -Iseconds) site deployed with the new release notes" \
      || echo "$(date -Iseconds) release notes written but the deploy failed"
  else
    echo "$(date -Iseconds) release notes written; deploy is manual (set KVASIR_RELEASE_AUTODEPLOY=1 to publish)"
  fi
fi

# Keep a month; the JSON is the evidence, the HTML is the artifact.
find "$LOG_DIR" -type f -mtime +31 -delete 2>/dev/null

echo "=== $(date -Iseconds) done ==="

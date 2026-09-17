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

#!/bin/zsh
# One day's run: collect, render, send. Keeps every artifact it produced, so a
# report that looked wrong can be read back against the facts it came from.
set -u
HERE="${0:A:h}"
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

if ! node "$HERE/collect.mjs" >"$JSON"; then
  # A failed collection is still news: say so in the group rather than go quiet.
  printf 'Kvasir · %s\n\nThe daily collection failed. See the runner log.\n' "$DAY" >"$SUMMARY"
  TELEGRAM_BOT_TOKEN="$TELEGRAM_BOT_TOKEN" TELEGRAM_CHAT_ID="$TELEGRAM_CHAT_ID" \
    node "$HERE/send.mjs" "$SUMMARY"
  echo "=== $(date -Iseconds) collect failed ==="
  exit 1
fi

node "$HERE/report.mjs" "$JSON" "$HTML" >"$SUMMARY" || exit 1
node "$HERE/send.mjs" "$SUMMARY" "$HTML" || exit 1

# Keep a month; the JSON is the evidence, the HTML is the artifact.
find "$LOG_DIR" -type f -mtime +31 -delete 2>/dev/null

echo "=== $(date -Iseconds) done ==="

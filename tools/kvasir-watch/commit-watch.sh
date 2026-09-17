#!/bin/zsh
# Between the daily reports: poll the watched repositories and post anything new.
# Exit 10 from commits.mjs means "there is something to send"; 0 means quiet.
set -u
HERE="${0:A:h}"
LOG_DIR="${KVASIR_WATCH_LOG:-$HERE/log}"
mkdir -p "$LOG_DIR"

ENV_FILE="${KVASIR_WATCH_ENV:-$HOME/project/any/.env}"
[[ -f "$ENV_FILE" ]] || { echo "no env file at $ENV_FILE" >>"$LOG_DIR/commits.log"; exit 2; }
set -a; source "$ENV_FILE"; set +a

OUT="$LOG_DIR/commits-latest.txt"
node "$HERE/commits.mjs" >"$OUT" 2>>"$LOG_DIR/commits.log"
code=$?

if [[ $code -eq 10 ]]; then
  node "$HERE/send.mjs" "$OUT" >>"$LOG_DIR/commits.log" 2>&1
  echo "$(date -Iseconds) sent $(wc -l <"$OUT" | tr -d ' ') lines" >>"$LOG_DIR/commits.log"
elif [[ $code -ne 0 ]]; then
  echo "$(date -Iseconds) poll failed ($code)" >>"$LOG_DIR/commits.log"
fi

# kvasir-watch

One report a day, in the Telegram group: what moved on each of the three tracks,
and what the machines actually say.

```
collect.mjs  → facts as JSON     (git, the MI250 agents, the public endpoints)
report.mjs   → report.html + a short summary
send.mjs     → summary as a message, the page as an attachment
daily.sh     → the three in order, logged and kept

commits.mjs       → new commits since last seen, across the watched repositories
commit-watch.sh   → polls every 20 minutes and posts only when there is news
```

## Why three tracks

Kvasir has three things in flight at once, and a note that shows only one of
them misreports the project:

| track | what the report reads |
| --- | --- |
| **Ring hardening** | the p4 agents on both MI250 machines: are they up, what stages do they hold, at what generation |
| **Settlement gateway** | commits under `solana/staking-service` and `p4bridge`, and whether the public endpoint answers |
| **Client app** | commits under `wallet/` and `apps/`, the desktop version, and whether the site is up |

A probe that fails is printed as a failure. Nothing is dropped for being
inconvenient: a report that silently omits an unreachable machine reads like
good news.

Known-deliberate states go in `config.json` under `notes` — that is how the
gateway's red endpoint reads as "down on purpose, pending the p4 port" instead
of an incident nobody noticed.

## The commit watch

Between the daily reports, `commit-watch.sh` posts commits as they land. Each
entry in `commitWatches` is one of two shapes:

| shape | endpoint | covers |
| --- | --- | --- |
| `{"repo": "owner/name"}` | repository events | pushes on **every branch**, private repos too (with a token that can read them) |
| `{"user": "login"}` | user events | that person across every repository — but GitHub exposes **public pushes only** here |

`tokenEnv` names the environment variable holding the token for that watch; the
value is read at the call and never logged. A watch's position is kept in
`state/commits.json`, and the first run of a new watch only records where it is
— it does not replay history into the group.

## Setup

```sh
npm install                       # links the p4 wire from ../../p4bridge
cp config.example.json config.json
$EDITOR config.json               # repo path, hosts, the three track globs
```

Credentials come from `~/project/any/.env` (override with `KVASIR_WATCH_ENV`):

```
TELEGRAM_BOT_TOKEN=…              # @BotFather
TELEGRAM_CHAT_ID=…                # negative for a group; -100… for a supergroup
```

To find the chat id: add the bot to the group, mention it once, then read
`message.chat.id` from `https://api.telegram.org/bot<token>/getUpdates`.

Run it once by hand:

```sh
./daily.sh                        # sends for real
node collect.mjs | node report.mjs - /tmp/kvasir.html    # dry run, no send
```

Then install the schedule (09:00 daily):

```sh
cp com.kvasir.watch.plist ~/Library/LaunchAgents/
launchctl load ~/Library/LaunchAgents/com.kvasir.watch.plist
```

## Access it needs

- `ssh` to the MI250 hosts by the aliases in `config.json` (key-based, no
  password prompt — the job runs unattended).
- A read-only INSPECT of each agent, over a tunnel it opens and closes itself.
  The agents bind loopback and this does not change that.
- Nothing writes to the engine. No model is loaded, no request is submitted.

## What it keeps

`log/<date>.json` is the evidence, `log/kvasir-<date>.html` is the artifact that
was sent, `log/<date>.log` is the runner's own output. Files older than 31 days
are removed on each run. `config.json` and `log/` are untracked.

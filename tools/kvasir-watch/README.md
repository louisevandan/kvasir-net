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

assess.mjs        → the day's judgement, written by Claude from the same facts
readiness.mjs     → rewrites the live blocks inside the S5 readiness review
calendar.mjs      → the week: meetings from an iCal feed, dates from plan.json
release-check.mjs → notices a shipped version and writes it into the site
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

| shape | how | covers |
| --- | --- | --- |
| `{"repo": "owner/name"}` | branch heads, then walk the ones that moved | every branch, private repositories included (with a token that can read them) |
| `{"user": "login"}` | user events | that person across every repository — but GitHub exposes **public pushes only** here |

Repository events look like the shorter path, but on a private repository
GitHub returns each push with an empty `commits` array: you learn that something
landed, not what. Branch heads do not have that hole.

`tokenEnv` names the environment variable holding the token for that watch; the
value is read at the call and never logged. A watch's position is kept in
`state/commits.json`, and the first run of a new watch only records where it is
— it does not replay history into the group.

## The readiness review

`readiness/en.html` and `readiness/ko.html` are the S5 readiness review. Most of
each file is reviewed text and the job never touches it; three marked blocks are
rewritten every morning:

| block | written by | holds |
| --- | --- | --- |
| `LIVE:STATUS` | `readiness.mjs` | the three tracks as the machines and the repository report them |
| `LIVE:CALENDAR` | `calendar.mjs` | this week — meetings from the calendar feed, dates from `plan.json` |
| `LIVE:ASSESSMENT` | `assess.mjs` | a judgement and the next steps, written from that morning's facts |

Both languages are rewritten together: a Korean reader and an English reader
must not end up with different numbers.

The assessment is a `claude -p` run over the collected JSON. It is given the
facts and nothing else, and it returns structured text rather than HTML — the
markup is ours, so a bad answer can be wrong but cannot break the page. When the
run fails, the previous assessment stays, clearly dated.

The English copy is what the group receives each morning. Not the claude.ai
artifact link: a headless `claude -p` authenticates into a different artifact
space and cannot update those pages, so `publish-readiness.sh` is there for a
person to run from an interactive session, and the daily job attaches the file.

Meetings come from a calendar's **secret iCal address**, because an unattended
job cannot hold an interactive Google session and that URL returns the same
events over plain HTTPS. Anyone holding it can read the calendar, so it lives in
the environment as `KVASIR_CALENDAR_ICS`, never in a committed file. In Google
Calendar: the calendar's ⋮ → Settings and sharing → Integrate calendar → Secret
address in iCal format. If it leaks, the Reset button on that same screen
invalidates it.

## Release notes

`release-check.mjs` reads the engine version off the running agents, the desktop
app's version from its manifest, and what the bridge catalog says is being
served. When one of them changes it appends an entry to the site's
`src/releases.json` — never inventing a release, only recording a value it read,
with a line saying where the value came from. Set
`KVASIR_RELEASE_AUTODEPLOY=1` (the daily job does) to publish the site after
writing one.

## Where it runs

On **MI250-02**, beside the fleet, under systemd user timers (`kvasir-watch-daily`
at 09:00 Asia/Seoul, `kvasir-watch-commits` every 20 minutes) with lingering
enabled so they run without a login session.

It used to run on a Mac under launchd and never actually fired: macOS lets a
launchd job stat a file on an external volume but not open it, so both jobs died
as `EX_CONFIG` with no output — indistinguishable from a job that was never
scheduled. Every message that reached the group before the move came from a
manual run. Running beside the fleet also removes the SSH tunnels: one agent is
on loopback here, and the other is one hop away over the LAN.

| piece | where |
| --- | --- |
| scripts | `~/kvasir-watch` on MI250-02 |
| repository | `~/kvasir-net-mirror`, a blobless clone, fetched before each run |
| credentials | `~/.kvasir-watch.env`, mode 600 |
| node | `~/.local/node`, user-local, no root |
| hop to MI250-01 | `~/.ssh/id_ed25519_kvasir_watch`, a key used for nothing else |

Two things do not work from there and stay with a person:

- **The assessment.** It shells out to `claude`, which is not installed on the
  host, so the section keeps its previous text and says when it was written.
- **Publishing the claude.ai artifacts and deploying the site.** Both need a
  session or the site toolchain.

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

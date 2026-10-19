# kvasir-admissions

A machine asks to join the ring, a person says yes, the ring is told.

```
bot ──POST /requests──▶ here ──Resend──▶ approver's inbox
                                           │ clicks
                                           ▼
                         GET /a/<id>   a page, not an approval
                                           │ presses the button
                                           ▼
                         POST /a/<id> ──▶ gate ──▶ bridge /api/admissions
```

## Why the click is two steps

A mail scanner or an inbox preview fetches the links in a message before a
human sees them. If the link itself admitted the wallet, the scanner would be
doing the approving and the one-time token would be spent by a machine reading
mail — silently, and in a way that looks from the outside like the approver
pressed it.

So `GET /a/<id>` only renders the request again, and the admission is a `POST`
that a prefetch does not make. It costs one more click and removes a whole
class of accidental approval.

## What the mail carries, and what it does not

The link carries a request id and a 32-byte one-time token. Together they
authorise exactly one approval of exactly one request, and nothing else — not
another wallet, not a second admission, not a re-approval after it is used.

The credential that talks to the ring (`P4_BRIDGE_TOKEN`) stays here. It is
never in a mail, never in a link, and never in a page.

## Limits, and why these ones

The mail is the thing that grants access, so the number that matters is how
many decisions can be in front of the approver at once — not how many rows the
disk could hold.

- **`ADMISSIONS_MAX_PENDING`** (50) — past this, a request is refused with 429
  and no mail is sent. An approver buried in requests approves the wrong one.
- **One live request per wallet** — asking twice returns the first request
  rather than mailing a second copy. Two mails for one decision, where using
  either silently kills the other, teaches a person to ignore both.
- **`ADMISSIONS_TTL_MINUTES`** (30) — how long a link is worth anything.
- Pruned on every intake: expired pending requests, requests whose mail never
  went out, and approvals older than `ADMISSIONS_KEEP_APPROVED_DAYS` (30). The
  bridge's admitted list is the authority on who is in; this is a record of how
  they got there, kept for a while.

## Running it

```sh
node server.mjs
```

| variable | | |
| --- | --- | --- |
| `RESEND_API_KEY` | required | the account that owns the sending domain |
| `ADMISSIONS_APPROVER` | required | who decides; one address |
| `P4_BRIDGE_TOKEN` | required | the service token the gate and bridge share |
| `ADMISSIONS_INTAKE_TOKEN` | required | what the bot presents to `POST /requests` |
| `ADMISSIONS_PUBLIC_URL` | | the address links are built from — must be reachable from the approver's mail client |
| `ADMISSIONS_FROM` | `admissions@reg.kvasir-ai.net` | a verified Resend sender |
| `KVASIR_GATE_URL` | `https://gate.kvasir-ai.net` | |
| `PORT` | 8795 | |
| `ADMISSIONS_STATE` | `state/requests.json` | |

It refuses to start without the four required ones. Discovering at the moment
someone is waiting that it cannot mail, or cannot admit, is worse than not
coming up.

## The intake token is not an admission control

It protects the approver's inbox, not the ring — approval still needs a human
press. Without it, anyone who finds the endpoint can point an unlimited stream
of real, correctly-signed mail at the approver, burn the sending domain's
reputation, and eventually get something wrong approved out of fatigue.

A wallet signature was considered instead and does not help: wallets are free
to make, so it authenticates the asker without bounding the asking.

## Requests

```sh
curl -fsS -X POST "$ADMISSIONS_PUBLIC_URL/requests" \
  -H "x-admissions-token: $ADMISSIONS_INTAKE_TOKEN" \
  -H 'content-type: application/json' \
  -d '{"wallet":"<base58>","profile":{"gpus":"RTX 6000 Ada","ram":"62Gi","lending":"24 GiB"},"requestedBy":"telegram:@someone"}'
```

`profile` is free-form and is shown to the approver as it arrives; it is what
the decision is made on. The shape the rest of the system uses for it is in
`llms-node.txt` under "Ask the operator to let this machine in".

Answers `{ok, id, expiresAt}`, or `{ok, id, reused:true}` when this wallet is
already waiting.

## Tests

```sh
node --test test/
```

Resend and the ring are both faked, so a test never sends mail and never admits
anything. What they check is the part that matters: that a `GET` changes
nothing, that a second press is not a second admission, that a forged token
admits nothing, that a ring outage does not consume the approval, and that the
limits above hold.

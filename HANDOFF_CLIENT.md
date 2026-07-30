# Client handoff — API key reissue, and syncing against the server

For whoever picks up the wallet clients next. Two things: what changed in the
apps, and what to know about the server they talk to, because the linker and
gateway sources you have locally are almost certainly behind what is deployed.

---

## Part 1 — What changed

### The bug this fixes

The credit API key is minted once by signing a gateway nonce, then cached on the
device. The gateway stores **only its hash**, so a key it no longer recognises
cannot be repaired by asking for it back — it has to be reminted.

That made the cache a trap. When the gateway's ledger was replaced (host
migration, fresh volume, revoked key), every client kept sending its stale
cached key and got `401 invalid API key` forever, with **no way out from inside
the app**. Recovery required clearing browser localStorage or reinstalling.

Worse, it hid itself: `ensureApiKey()` returns the cached key without contacting
the server, and `/api/credits/deposit` authenticates by *wallet signature*, not
by API key. So top-ups kept succeeding while balance reads kept 401ing — the
account looked funded and broken at the same time.

### What was added

A **Reissue key** action in each app's settings that drops the cached key and
mints a fresh one. Credit balance is held against the wallet, not the key, so
reissuing costs nothing.

| Platform | Files |
| --- | --- |
| Desktop | `wallet/desktop/src/creditKey.ts` (new), `screens/settings.tsx`, `screens/inference.tsx`, `i18n.tsx` |
| iOS | `App/StakingStore.swift`, `App/NodeSettings.swift`, `App/InferenceStore.swift`, `App/Localization.swift` |
| Android | `wallet/.../WalletViewModel.kt`, `NodeSettings.kt`, `KeyStore.kt`, `Localization.kt` |

Notes on the shape of it:

- **Desktop**: minting/caching/recovery moved out of `inference.tsx` into
  `creditKey.ts` so Settings and Inference share one implementation.
  `ensureApiKey` / `mintApiKey` / `reissueApiKey` / `cachedApiKey` /
  `clearApiKey`.
- **iOS**: `StakingStore.reissueCreditApiKey()` — put there, not on
  `InferenceStore`, because `NodeSettingsView` already holds a `StakingStore`
  and `InferenceStore` is constructed per-screen. `InferenceStore` also gained
  its own `reissueApiKey()` for the inference screen.
- **Android**: `WalletViewModel.reissueCreditApiKey()`, plus
  `KeyStore.deleteApiKey()` which did not exist.
- **Strings**: added to `ko` and `en` only. All three apps fall back to English
  for missing keys, so the other locales render English rather than the raw key.
  Translate at leisure.

### What was NOT done — read this before you build on it

**None of the three apps were compiled.** The desktop app was type-checked only
indirectly, by building the `gate` Docker image (its first stage runs
`npm run build:web`, which passed). iOS and Android were **not built at all** —
there is no Xcode or Android SDK on the machine this was written on.

So before trusting the mobile changes:

```bash
# iOS
cd wallet/ios && xcodegen generate && xcodebuild -scheme … build

# Android
cd wallet/android && ./gradlew assembleDebug
```

Symbols were checked by reading (`Strings(vm.language)` and `GREEN` match
existing usage in the same file; `CreditService(baseUrl:)` matches the
initialiser; `KeyStore.deleteApiKey` exists on iOS and was added on Android),
but that is not a compiler.

### Worth considering next

The reissue button is a manual escape hatch. The failure could heal itself: if
`GET /api/credits/balance` returns 401, the cached key is definitively dead —
clear it and remint transparently. That turns a support conversation into
nothing at all. Deliberately not done here, because it changes behaviour on
every 401 including transient ones, and that deserves its own decision.

---

## Part 2 — Syncing against the server

**The `linker` and `gateway` sources in a typical client checkout are old.** The
deployment changed shape substantially. If you reason about client behaviour
from a stale server checkout you will reach wrong conclusions — that is exactly
what happened while debugging this bug.

### The current architecture

```
client (Electron / iOS / Android / browser)
        │
        ▼  Cloudflare Tunnel — the only public path
  gate.kvasir-ai.net → :8791   solana/staking-service
      wallet web app + /api/node/*, /api/stake, /api/pay/*,
      /api/credits/*, /api/inference, /api/config
                        │ x-linkcpp-service-token
  hub.kvasir-ai.net  → :19000  controller/hub.py
      operator auth, settlement view, linker SPA at /linker
                        │ delegation
                          :19001  linker  ← never published
```

Clients talk to **gate only**. `hub` is the operator surface and is locked
behind a wallet session; `linker` is not reachable from outside the host at all.

### Which repository is which

| Component | Repo / branch | Reference commit |
| --- | --- | --- |
| Wallets, `staking-service`, `controller/hub.py` | `louisevandan/kvasir-net` @ `kvasir-net` | `97c8398` |
| linker (control plane) | `hikaMaeng/linkcpp` @ `convertarchitecture` | `d59438d5` |

These are **two separate repositories with unrelated histories** — there is no
merge base between them. Do not try to merge; treat linker as an external
service consumed over its API.

If your `kvasir-net` checkout predates `d437efa` ("Kvasir — clean reboot"), it
is on an abandoned history line. Re-clone rather than pull.

### What changed server-side that affects clients

- **`controller/hub.py` no longer implements the control plane.** Nodes,
  controllers, planning, model loading and inference are delegated to linker.
  It kept auth, the settlement view, and — deliberately — the MoE expert market,
  external-controller registration, and the stage/ring proxy.
- **The hub is locked by default.** No wallet session, no access; a 401 on a
  browser navigation renders a lock screen rather than JSON. Sessions have a
  5-minute idle life, slid forward by `/api/auth/touch` on real user input.
  Irrelevant to wallet clients (they never touch `hub`), relevant if you point
  a browser at it.
- **The model list is not local to gate.** `fetchModelsFrom()` polls the hub's
  `/api/controllers` and surfaces only controllers that are `runtime_loaded`.
  It swallows connection errors and returns `[]`, so a hub that is unreachable
  or answering 401 shows up in the app as an **empty model dropdown with nothing
  in any log**. If the dropdown is empty, check the server side before the
  client.
- **Two payment paths coexist.** Pay-per-request (`/api/pay/quote` → sign →
  `/api/inference`, no API key) and credit accounts (`/api/credits/*` → API key
  → `/v1/chat/completions`). The desktop inference screen uses the credit path;
  `topUp()` calls `ensureApiKey()` first because deposit requires the wallet to
  be whitelisted.

### Server details that bite

- **Challenge nonces live in gateway process memory with a 5-minute TTL.**
  Restarting the gateway between issuing a challenge and verifying the signature
  invalidates it. If registration fails right after a deploy, retry before
  investigating. This also means the gateway cannot be horizontally scaled as
  written — the instance that issued the nonce must be the one that verifies it.
- **The settlement ledger is a Docker named volume** (`gateway-data`,
  `/app/data/positions.json`): stake positions, node registry, credit accounts,
  API key hashes. A host migration that misses this volume drops every issued
  key — which is the scenario the reissue button exists for.

### Deployment references

- `DEPLOY_GATE.md` — standing up a gate host, and the settings whose absence
  fails silently.
- `GATEWAY_GUIDE.md` — architecture, token facts, reward economics, what each
  service owns.

---

## Part 3 — One more client bug, already fixed

`wallet/desktop/electron/main.cjs` threw instead of picking an endpoint on
mainnet:

```js
(C.clusters[network] || C.clusters[C.defaultCluster]).rpcUrl
```

The app's `Network` is `'devnet' | 'mainnet'`, while `wallet-constants.json`
keys clusters by Solana's names (`mainnet-beta`), and there is no
`defaultCluster` key for the fallback to find. Fixed at the point of use rather
than in the shared spec, since iOS and Android read that file too.

`browserWallet.ts` was unaffected — it keys its own map by the app's names and
falls back to devnet.

**Devnet never hit this**, which is why it went unnoticed. Worth a look if
mainnet is on the roadmap: the same `'mainnet'` vs `'mainnet-beta'` split may
exist elsewhere.

# linkcpp token tooling (devnet)

Node scripts that create and manage the linkcpp SPL token on Solana **devnet**.
No Solana CLI required — everything runs via `@solana/web3.js` + `@solana/spl-token`.

## Setup

```bash
cd solana/token
npm install
```

## Create the mint

Edit token params in [`config.js`](config.js) if needed (name, symbol, decimals,
initial supply), then:

```bash
npm run create-mint
```

This will:

1. Create/load an admin keypair in `.keys/admin.json` (gitignored). The admin is
   payer + mint authority + freeze authority + treasury owner for the devnet phase.
2. Airdrop devnet SOL to the admin if its balance is low.
3. Create the SPL mint with the configured decimals.
4. Create the treasury associated token account and mint the initial supply to it.
5. Write [`../../wallet/shared-spec/token.devnet.json`](../../wallet/shared-spec/token.devnet.json)
   — the single source of truth the iOS/Android wallets read.

## Other commands

```bash
npm run show                    # print on-chain mint supply + treasury balance
npm run airdrop -- <pubkey> 1   # airdrop 1 SOL to any devnet pubkey
npm run add-metadata            # optional/deferred: on-chain Metaplex metadata
```

## Notes

- **Keys stay local.** `.keys/` is gitignored. The admin key is a devnet-only
  operator key; never reuse it or its mnemonic on mainnet.
- Public devnet's faucet is rate-limited. If `create-mint` reports an airdrop
  failure, fund the printed admin address via https://faucet.solana.com and re-run.
- To point at a different RPC: `LINKCPP_RPC_URL=... npm run create-mint`.

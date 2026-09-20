import { useEffect } from "react";
import type { ReactNode } from "react";
import { Container, Pill } from "./ui";
import Nav from "./Nav";
import Footer from "./Footer";
import { LINKS } from "../content";
import { useT, useLang } from "../i18n/provider";

/* ==========================================================================
   Terms of use & privacy notice (/legal). English only in every locale — a
   single authoritative text avoids translation drift in legal wording.
   Every statement is grounded in the code as of the date below:
   solana/staking-service (gate), p4bridge (the bridge that replaced the retired
   hub control plane), wallet/*, and this site. Update the date and the facts
   together when those change.
   ========================================================================== */

const UPDATED = "2026-09-21";
const CONTACT = "sales@newtype-ai.com";

function Section({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <section id={id} className="scroll-mt-24 border-t border-line pt-10">
      <h2 className="text-2xl font-semibold text-ink">{title}</h2>
      <div className="mt-4 space-y-4 text-[15px] leading-relaxed text-ink-muted">{children}</div>
    </section>
  );
}

function List({ items }: { items: ReactNode[] }) {
  return (
    <ul className="list-disc space-y-2 pl-5 marker:text-ink-faint">
      {items.map((it, i) => (
        <li key={i}>{it}</li>
      ))}
    </ul>
  );
}

const code = "rounded bg-surface-2/70 px-1.5 py-0.5 font-mono text-[13px] text-ink ring-1 ring-line";

export default function LegalPage() {
  const t = useT();
  const { lang } = useLang();
  useEffect(() => {
    window.scrollTo(0, 0);
  }, []);
  useEffect(() => {
    document.title = "Terms of use & privacy — Kvasir";
  }, []);

  return (
    <div className="min-h-screen">
      <Nav />
      <Container className="pt-28 pb-20 sm:pt-36">
        <div className="max-w-3xl">
          <Pill tone="brand">{t.footer.legal}</Pill>
          <h1 className="mt-5 text-4xl font-semibold leading-tight tracking-tight text-ink sm:text-5xl">
            Terms of use &amp; privacy
          </h1>
          <p className="mt-5 text-lg leading-relaxed text-ink-muted">
            Kvasir is a devnet preview. This page explains the terms for using the website, the gateway
            (<span className="font-mono text-base">gate.kvasir-ai.net</span>), the inference bridge behind it
            and the Kvasir Wallet apps, and what data they handle.
          </p>
          <p className="mt-3 text-sm text-ink-faint">
            Last updated {UPDATED} · Operator: Kvasir AI Network (the licensor named in the{" "}
            <a className="text-brand-300 hover:underline" href={LINKS.github} target="_blank" rel="noreferrer">
              source license
            </a>
            ) · Contact:{" "}
            <a className="text-brand-300 hover:underline" href={`mailto:${CONTACT}`}>
              {CONTACT}
            </a>
          </p>
          {lang !== "en" && (
            <p className="mt-3 text-sm text-ink-faint">This page is provided in English only.</p>
          )}
        </div>

        <div className="mt-14 max-w-3xl space-y-12">
          <Section id="terms" title="Terms of use">
            <List
              items={[
                <>
                  <strong className="text-ink">Devnet only.</strong> Everything runs on Solana devnet. KVR is a devnet
                  utility token with no monetary value. It is not an investment, and nothing on this site or in the
                  apps is an offer, a price, or a promise of financial return. Devnet balances, stakes and ledgers may
                  be reset at any time.
                </>,
                <>
                  <strong className="text-ink">Preview service.</strong> The gateway, hub and test fleet are provided
                  as is, without any uptime or performance guarantee, and may change or stop without notice.
                </>,
                <>
                  <strong className="text-ink">Your wallet.</strong> Wallet keys and the recovery phrase are created
                  and stored on your device. Nobody at Kvasir can see or reset them; if you lose the phrase, the
                  account cannot be recovered.
                </>,
                <>
                  <strong className="text-ink">Staking and credits.</strong> On devnet, staked KVR and prepaid
                  inference credits are held in the gateway&apos;s treasury wallet and tracked in the gateway&apos;s
                  ledger until an on-chain staking program ships. Reward and pricing parameters are devnet settings
                  and can change.
                </>,
                <>
                  <strong className="text-ink">Running a node.</strong> You are responsible for your hardware,
                  power, network and for complying with the licenses of the models you serve.
                </>,
                <>
                  <strong className="text-ink">AI output.</strong> Model responses can be wrong or incomplete. Do not
                  rely on them for medical, legal, financial or safety decisions.
                </>,
                <>
                  <strong className="text-ink">Acceptable use.</strong> Do not use the network for unlawful content,
                  to attack or overload the service or other nodes, to abuse the faucet, or to access accounts that
                  are not yours.
                </>,
                <>
                  <strong className="text-ink">Software license.</strong> The engine source is published under the
                  Business Source License 1.1. Non-monetized internal use is permitted; hosted, embedded or
                  revenue-generating use requires a commercial license. See the LICENSE file in the repository.
                </>,
                <>
                  <strong className="text-ink">Liability.</strong> To the extent the law allows, Kvasir is not liable
                  for losses arising from use of this devnet preview.
                </>,
              ]}
            />
          </Section>

          <Section id="privacy" title="Privacy notice">
            <h3 className="pt-2 text-lg font-semibold text-ink">This website</h3>
            <List
              items={[
                <>
                  No analytics, advertising or tracking scripts. Your language choice is kept in your browser&apos;s
                  local storage (<code className={code}>kvasir-lang</code>).
                </>,
                <>
                  The site is hosted on Cloudflare, which processes request data such as IP addresses to deliver and
                  protect the site. Desktop installers are downloaded from Cloudflare R2.
                </>,
              ]}
            />

            <h3 className="pt-2 text-lg font-semibold text-ink">Gateway and hub</h3>
            <List
              items={[
                <>
                  <strong className="text-ink">Wallet activity:</strong> wallet addresses, stake amounts and times,
                  consumed transaction signatures, faucet request times, credit balances and spend, and usage records
                  (wallet, model and token counts; the most recent 1,000 entries).
                </>,
                <>
                  <strong className="text-ink">Pay-per-call inference:</strong> the prompt and the generated answer are
                  stored together with the paying wallet address. They are currently kept without automatic deletion.
                  Requests made with a credits API key (<code className={code}>/v1/chat/completions</code>) record
                  token counts only, not content. Do not put personal or sensitive information in prompts.
                </>,
                <>
                  <strong className="text-ink">API keys:</strong> stored only as a SHA-256 hash, with the wallet, a
                  label and the creation time.
                </>,
                <>
                  <strong className="text-ink">Operator sign-in:</strong> operators sign in with a wallet signature
                  (Sign-In With Solana). Optional 2FA stores a TOTP secret on the server; backup codes are stored
                  hashed. Session cookies (<code className={code}>kvr_admin_session</code>,{" "}
                  <code className={code}>linkcpp_session</code>) are used only for signed-in operators.
                </>,
                <>
                  <strong className="text-ink">Nodes:</strong> a registered node reports its device type, operating
                  system, accelerator, backend, measured throughput and a device label, which may include the
                  device name or hostname; mobile node IDs are derived from the device&apos;s app-vendor identifier.
                  The public node list shows labels with shortened owner addresses.
                </>,
                <>
                  <strong className="text-ink">IP addresses:</strong> the gateway records an IP address only when it
                  refuses a key that is restricted to certain addresses. Cloudflare processes connection data for the
                  public endpoints.
                </>,
              ]}
            />

            <h3 className="pt-2 text-lg font-semibold text-ink">Wallet apps</h3>
            <List
              items={[
                <>
                  Keys stay on the device: encrypted browser storage on the web, an encrypted file on desktop, the
                  Keychain on iOS and encrypted preferences on Android. The web wallet also keeps your credits API key
                  and your last 100 chat messages in the browser.
                </>,
              ]}
            />

            <h3 className="pt-2 text-lg font-semibold text-ink">Third parties and public data</h3>
            <List
              items={[
                <>
                  Solana devnet RPC. Transactions on Solana are public and permanent; they cannot be deleted.
                </>,
                <>Cloudflare (hosting, tunnel and downloads), Hugging Face (model downloads by nodes) and GitHub (source code).</>,
              ]}
            />

            <h3 className="pt-2 text-lg font-semibold text-ink">Retention and your requests</h3>
            <p>
              Gateway records are currently kept until the devnet ledger is reset. To ask for a copy or deletion of
              off-chain records tied to your wallet address, email{" "}
              <a className="text-brand-300 hover:underline" href={`mailto:${CONTACT}`}>
                {CONTACT}
              </a>{" "}
              from a channel where you can also prove control of that wallet (for example, a signed message).
              On-chain data cannot be erased.
            </p>
          </Section>
        </div>
      </Container>
      <Footer />
    </div>
  );
}

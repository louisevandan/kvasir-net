import { useState } from "react";
import { SolanaIcon } from "./icons";
import { LINKS } from "../content";

/* ==========================================================================
   Header control that links to the KVR SPL token on the Solana explorer, so
   anyone can inspect on-chain KVR transaction activity. Replaces the old
   header GitHub icon. Small dropdown: Devnet (live) / Mainnet clusters.
   Self-contained (own open state + click-away). Reused across every header.
   ========================================================================== */
const CLUSTERS = [
  { label: "Devnet", href: LINKS.explorerDevnet },
  { label: "Mainnet", href: LINKS.explorerMainnet },
];

function Chevron({ open }: { open: boolean }) {
  return (
    <svg
      width={13}
      height={13}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={`transition-transform duration-200 ${open ? "rotate-180" : ""}`}
    >
      <path d="M6 9l6 6 6-6" />
    </svg>
  );
}

export default function KvrExplorer({ className = "" }: { className?: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={`relative ${className}`}>
      <button
        onClick={() => setOpen((v) => !v)}
        aria-label="KVR on the Solana explorer"
        aria-expanded={open}
        className="group inline-flex h-10 w-full items-center justify-center gap-2 rounded-full px-3 text-sm text-ink-muted ring-1 ring-line transition-colors hover:bg-white/5 hover:text-ink"
      >
        <SolanaIcon width={18} height={18} />
        <span className="hidden lg:inline">KVR</span>
        <Chevron open={open} />
      </button>
      {open && (
        <>
          <button
            aria-hidden
            tabIndex={-1}
            className="fixed inset-0 z-40 cursor-default"
            onClick={() => setOpen(false)}
          />
          <div className="absolute right-0 top-full z-50 mt-1 min-w-[10rem] overflow-hidden rounded-xl border border-line bg-surface-2/95 py-1 shadow-glow backdrop-blur">
            <div className="px-4 pb-1 pt-1.5 text-[0.62rem] font-semibold uppercase tracking-wider text-ink-faint">
              KVR on Solana
            </div>
            {CLUSTERS.map((c) => (
              <a
                key={c.label}
                href={c.href}
                target="_blank"
                rel="noreferrer"
                onClick={() => setOpen(false)}
                className="flex items-center gap-2.5 px-4 py-2 text-sm text-ink-muted transition-colors hover:bg-white/5 hover:text-ink"
              >
                <SolanaIcon width={14} height={14} className="text-brand-400" />
                {c.label}
              </a>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

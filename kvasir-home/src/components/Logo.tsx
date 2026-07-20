export function LogoMark({ size = 32 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      fill="none"
      aria-hidden
      className="shrink-0"
    >
      <defs>
        <linearGradient id="lm" x1="0" y1="0" x2="64" y2="64" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#ff4d97" />
          <stop offset="1" stopColor="#a855f7" />
        </linearGradient>
      </defs>
      <rect width="64" height="64" rx="16" fill="#0e1015" />
      <rect
        x="1"
        y="1"
        width="62"
        height="62"
        rx="15"
        fill="none"
        stroke="url(#lm)"
        strokeOpacity="0.4"
        strokeWidth="1.5"
      />
      <path
        d="M22 16 L22 48 M22 32 L40 16 M22 32 L40 48"
        fill="none"
        stroke="url(#lm)"
        strokeWidth="5.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <circle cx="40" cy="16" r="4.5" fill="#ff4d97" />
      <circle cx="40" cy="48" r="4.5" fill="#a855f7" />
      <circle cx="22" cy="32" r="4.5" fill="#ff4d97" />
    </svg>
  );
}

export function Wordmark({ size = 32 }: { size?: number }) {
  return (
    <div className="flex items-center gap-2.5">
      <LogoMark size={size} />
      <span className="text-lg font-semibold tracking-tight text-ink">
        Kvasir
      </span>
    </div>
  );
}

import {
  useEffect,
  useRef,
  useState,
  type AnchorHTMLAttributes,
  type ReactNode,
} from "react";

/* --------------------------------------------------------------------------
   Container — consistent page gutters + max width
   -------------------------------------------------------------------------- */
export function Container({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={`mx-auto w-full max-w-[82.944rem] px-5 sm:px-8 ${className}`}>
      {children}
    </div>
  );
}

/* --------------------------------------------------------------------------
   Button — brand-consistent CTA styles
   -------------------------------------------------------------------------- */
type ButtonProps = AnchorHTMLAttributes<HTMLAnchorElement> & {
  variant?: "primary" | "secondary" | "ghost";
  size?: "md" | "lg";
  children: ReactNode;
};

export function Button({
  variant = "primary",
  size = "md",
  className = "",
  children,
  ...props
}: ButtonProps) {
  const sizes = {
    md: "h-10 px-4 text-sm",
    lg: "h-12 px-6 text-[0.95rem]",
  };
  const variants = {
    primary:
      "bg-brand-500 text-white font-semibold shadow-glow hover:bg-brand-400 hover:-translate-y-0.5",
    secondary:
      "bg-surface-2 text-ink ring-1 ring-line hover:bg-surface-3 hover:ring-brand-500/40",
    ghost:
      "text-ink-muted hover:text-ink hover:bg-white/5",
  };
  return (
    <a
      className={`group inline-flex items-center justify-center gap-2 rounded-full transition-all duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-400 focus-visible:ring-offset-2 focus-visible:ring-offset-bg ${sizes[size]} ${variants[variant]} ${className}`}
      {...props}
    >
      {children}
    </a>
  );
}

/* --------------------------------------------------------------------------
   Pill / eyebrow label
   -------------------------------------------------------------------------- */
export function Pill({
  children,
  tone = "brand",
  className = "",
}: {
  children: ReactNode;
  tone?: "brand" | "muted" | "caution" | "positive";
  className?: string;
}) {
  const tones = {
    brand: "text-brand-300 ring-brand-500/25 bg-brand-500/10",
    muted: "text-ink-muted ring-line bg-surface-2",
    caution: "text-caution ring-caution/25 bg-caution/10",
    positive: "text-positive ring-positive/25 bg-positive/10",
  };
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-medium ring-1 ${tones[tone]} ${className}`}
    >
      {children}
    </span>
  );
}

/* --------------------------------------------------------------------------
   Card — surface panel
   -------------------------------------------------------------------------- */
export function Card({
  children,
  className = "",
  interactive = false,
}: {
  children: ReactNode;
  className?: string;
  interactive?: boolean;
}) {
  return (
    <div
      className={`rounded-2xl bg-surface-2/70 ring-1 ring-line ${
        interactive
          ? "transition-all duration-300 hover:ring-brand-500/40 hover:bg-surface-2 hover:-translate-y-1"
          : ""
      } ${className}`}
    >
      {children}
    </div>
  );
}

/* --------------------------------------------------------------------------
   SectionHeading — eyebrow + title + optional lede
   -------------------------------------------------------------------------- */
export function SectionHeading({
  eyebrow,
  title,
  lede,
  align = "left",
  titleClassName = "",
}: {
  eyebrow?: string;
  title: ReactNode;
  lede?: ReactNode;
  align?: "left" | "center";
  titleClassName?: string;
}) {
  return (
    <div className={align === "center" ? "mx-auto max-w-2xl text-center" : "max-w-2xl"}>
      {eyebrow && (
        <div
          className={`mb-3 text-xs font-semibold uppercase tracking-[0.2em] text-brand-400 ${
            align === "center" ? "" : ""
          }`}
        >
          {eyebrow}
        </div>
      )}
      <h2 className={`text-3xl font-semibold text-ink sm:text-4xl ${titleClassName}`}>{title}</h2>
      {lede && (
        <p className="mt-4 text-base leading-relaxed text-ink-muted sm:text-lg">
          {lede}
        </p>
      )}
    </div>
  );
}

/* --------------------------------------------------------------------------
   Section wrapper — vertical rhythm + anchor
   -------------------------------------------------------------------------- */
export function Section({
  id,
  children,
  className = "",
}: {
  id?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section id={id} className={`py-20 sm:py-28 ${className}`}>
      {children}
    </section>
  );
}

/* --------------------------------------------------------------------------
   Reveal — fade/slide in on scroll (IntersectionObserver)
   -------------------------------------------------------------------------- */
export function Reveal({
  children,
  delay = 0,
  className = "",
}: {
  children: ReactNode;
  delay?: number;
  className?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [shown, setShown] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        entries.forEach((e) => {
          if (e.isIntersecting) {
            setShown(true);
            io.disconnect();
          }
        });
      },
      { threshold: 0.12, rootMargin: "0px 0px -8% 0px" }
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  return (
    <div
      ref={ref}
      style={{ transitionDelay: `${delay}ms` }}
      className={`transition-all duration-700 ease-out ${
        shown ? "translate-y-0 opacity-100" : "translate-y-6 opacity-0"
      } ${className}`}
    >
      {children}
    </div>
  );
}

import { useEffect, useRef, useState } from "react";
import { NODE_SPLIT } from "../content";
import { useT } from "../i18n/provider";

/* Total layers of the demoed model (Qwen3.5-122B). */
const TOTAL_LAYERS = 49;
const TIER_MULT: Record<string, number> = { S: 1.5, A: 1.25, B: 1.0, C: 0.7 };

/* Ring layout in the 600 x 440 viewBox — four device nodes on a ring, no master. */
const CENTER = { x: 300, y: 215 };
const NODES = [
  { x: 300, y: 66 }, // top
  { x: 500, y: 214 }, // right
  { x: 300, y: 362 }, // bottom
  { x: 100, y: 214 }, // left
];
const SEGMENTS = [
  [0, 1],
  [1, 2],
  [2, 3],
  [3, 0],
];

function useReducedMotion() {
  const [reduced, setReduced] = useState(false);
  useEffect(() => {
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    setReduced(mq.matches);
    const on = () => setReduced(mq.matches);
    mq.addEventListener?.("change", on);
    return () => mq.removeEventListener?.("change", on);
  }, []);
  return reduced;
}

/* Illustrative KVR accrual — each node earns at a rate set by its layer share
   and performance tier. Schematic, not a real network total. */
function useAccruals(reduced: boolean) {
  const [vals, setVals] = useState(() =>
    NODE_SPLIT.map((n) => (n.layers / TOTAL_LAYERS) * 4)
  );
  const raf = useRef<number>(0);
  const last = useRef<number>(0);

  useEffect(() => {
    if (reduced) return;
    const tick = (t: number) => {
      if (!last.current) last.current = t;
      const dt = (t - last.current) / 1000;
      last.current = t;
      setVals((prev) =>
        prev.map((v, i) => {
          const n = NODE_SPLIT[i];
          const rate = (n.layers / TOTAL_LAYERS) * (TIER_MULT[n.tier] ?? 1) * 1.6;
          return v + rate * dt;
        })
      );
      raf.current = requestAnimationFrame(tick);
    };
    raf.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf.current);
  }, [reduced]);

  return vals;
}

/* Arc between two node centers, bowed outward from the ring center. */
function ringPath(a: { x: number; y: number }, b: { x: number; y: number }) {
  const mx = (a.x + b.x) / 2;
  const my = (a.y + b.y) / 2;
  const cx = mx + 0.42 * (mx - CENTER.x);
  const cy = my + 0.42 * (my - CENTER.y);
  return `M ${a.x} ${a.y} Q ${cx} ${cy} ${b.x} ${b.y}`;
}

export default function Topology() {
  const t = useT();
  const reduced = useReducedMotion();
  const accruals = useAccruals(reduced);

  return (
    <figure className="relative mx-auto w-[90%]">
      {/* ambient glow */}
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 -z-10 blur-3xl"
        style={{
          background:
            "radial-gradient(55% 55% at 50% 50%, rgba(255,61,139,0.20), transparent 70%)",
        }}
      />
      <div className="rounded-2xl bg-surface/60 ring-1 ring-line backdrop-blur-sm">
        <svg
          viewBox="0 0 600 440"
          className="w-full"
          role="img"
          aria-label="One model split into contiguous layer windows around a ring of devices — a GPU, a CPU, an NPU and a phone. Each node runs its layers and passes only the hidden-state boundary to its neighbor around the ring; there is no central master. Each node earns KVR weighted by its layer share."
        >
          <defs>
            <linearGradient id="edge" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" stopColor="#ff3d8b" />
              <stop offset="1" stopColor="#a855f7" />
            </linearGradient>
            <radialGradient id="ringGlow" cx="0.5" cy="0.5" r="0.5">
              <stop offset="0" stopColor="#ff3d8b" stopOpacity="0.28" />
              <stop offset="1" stopColor="#ff3d8b" stopOpacity="0" />
            </radialGradient>
            <filter id="soft" x="-50%" y="-50%" width="200%" height="200%">
              <feGaussianBlur stdDeviation="2.2" />
            </filter>
          </defs>

          {/* ring connectors */}
          {SEGMENTS.map(([a, b], i) => {
            const d = ringPath(NODES[a], NODES[b]);
            return (
              <g key={`seg-${i}`}>
                <path
                  d={d}
                  fill="none"
                  stroke="url(#edge)"
                  strokeOpacity={0.5}
                  strokeWidth={1.6}
                  strokeDasharray="4 6"
                  className={reduced ? "" : "animate-flow"}
                />
                {!reduced && (
                  <>
                    {/* hidden-state bundle travelling around the ring */}
                    <circle r="3.6" fill="#ff5fa2" filter="url(#soft)">
                      <animateMotion
                        dur="3.6s"
                        repeatCount="indefinite"
                        path={d}
                        begin={`${i * 0.9}s`}
                      />
                    </circle>
                    <circle r="2.2" fill="#c084fc">
                      <animateMotion
                        dur="3.6s"
                        repeatCount="indefinite"
                        path={d}
                        begin={`${i * 0.9 + 1.8}s`}
                      />
                    </circle>
                  </>
                )}
              </g>
            );
          })}

          {/* ring center label (not a node — no master) */}
          <g>
            <circle cx={CENTER.x} cy={CENTER.y} r="64" fill="url(#ringGlow)" />
            <rect
              x={CENTER.x - 76}
              y={CENTER.y - 26}
              width="152"
              height="52"
              rx="13"
              fill="#14161d"
              stroke="#ff3d8b"
              strokeOpacity="0.45"
            />
            <text
              x={CENTER.x}
              y={CENTER.y - 3}
              textAnchor="middle"
              className="fill-white"
              style={{ font: "700 14px var(--font-sans)" }}
            >
              Kvasir AI
            </text>
            <text
              x={CENTER.x}
              y={CENTER.y + 15}
              textAnchor="middle"
              style={{ font: "500 10.5px var(--font-mono)", fill: "#ff8dbe" }}
            >
              {t.hero.ringCenter}
            </text>
          </g>

          {/* device nodes on the ring */}
          {NODES.map((p, i) => {
            const node = NODE_SPLIT[i];
            const x0 = p.x - 78;
            const y0 = p.y - 33;
            return (
              <g key={`node-${i}`}>
                <rect
                  x={x0}
                  y={y0}
                  width="156"
                  height="66"
                  rx="13"
                  fill="#0e1015"
                  stroke="#262a35"
                />
                <text
                  x={x0 + 14}
                  y={y0 + 20}
                  style={{ font: "600 13px var(--font-sans)", fill: "#f4f5f8" }}
                >
                  {node.id}
                </text>
                <rect
                  x={x0 + 156 - 40}
                  y={y0 + 8}
                  width="30"
                  height="17"
                  rx="8.5"
                  fill="rgba(255,61,139,0.12)"
                  stroke="rgba(255,61,139,0.4)"
                />
                <text
                  x={x0 + 156 - 25}
                  y={y0 + 20}
                  textAnchor="middle"
                  style={{ font: "600 11px var(--font-mono)", fill: "#ff8dbe" }}
                >
                  {node.tier}
                </text>
                <rect
                  x={x0 + 14}
                  y={y0 + 30}
                  width="128"
                  height="5"
                  rx="2.5"
                  fill="#1b1e27"
                />
                <rect
                  x={x0 + 14}
                  y={y0 + 30}
                  width={128 * (node.layers / TOTAL_LAYERS)}
                  height="5"
                  rx="2.5"
                  fill="url(#edge)"
                />
                <text
                  x={x0 + 14}
                  y={y0 + 52}
                  style={{ font: "500 10px var(--font-mono)", fill: "#6b7183" }}
                >
                  {node.layers} layers
                </text>
                <text
                  x={x0 + 142}
                  y={y0 + 52}
                  textAnchor="end"
                  style={{ font: "600 10.5px var(--font-mono)", fill: "#3cbf8e" }}
                >
                  +{accruals[i].toFixed(1)} KVR
                </text>
              </g>
            );
          })}
        </svg>
      </div>
      <figcaption className="mt-3 px-1 text-center text-xs text-ink-faint">
        {t.hero.topologyCaption}
      </figcaption>
    </figure>
  );
}

import { useEffect } from "react";
import type { ReactNode } from "react";
import { Container, Card, Pill } from "./ui";
import Nav from "./Nav";
import Footer from "./Footer";

/* ==========================================================================
   Who builds this, and under what company (/team).

   English only, like /legal and /install: one authoritative text. Names,
   roles and a corporate structure are the last things that should differ
   between translations.

   The portraits are line engravings, not photographs, and deliberately so.
   Publishing the faces of a small team is a privacy decision none of us
   should have to make to be taken seriously — and a generated photorealistic
   face standing in for a real person is worse than no photo, because it is a
   small lie on the page where someone decides whether to trust us. Drawn line
   work cannot be mistaken for either, and each carries a motif from the work
   that person does. The page says as much rather than leaving it to be found.
   ========================================================================== */

const UPDATED = "2026-09-21";

type Person = {
  name: string;
  role: string;
  based: string;
  avatar: string;
  /** The motif drawn behind them, so the illustration reads as deliberate. */
  motif: string;
  body: string;
};

const PEOPLE: Person[] = [
  {
    name: "Guy Jaber",
    role: "Founder",
    based: "United States",
    avatar: "/team/guy.webp",
    motif: "Lines radiating outward — reach",
    body:
      "Silicon Valley background. Runs business development, marketing and capital: "
      + "seeding the operator network, the narrative, and relationships with investors "
      + "and exchanges.",
  },
  {
    name: "Kiwan Maeng",
    role: "Founder · Chief Engineering",
    based: "Korea",
    avatar: "/team/kiwan.webp",
    motif: "Concentric rings — the runtime loop",
    body:
      "Distributed AI runtime engineering, with depth across inference systems. Leads "
      + "the swarm runtime and the node platform — the parts that decide whether a model "
      + "too large for any one machine actually serves.",
  },
  {
    name: "Anver Layshev",
    role: "Business Development · Marketing",
    based: "United Arab Emirates",
    avatar: "/team/anver.webp",
    motif: "Rising bars — a market forming",
    body:
      "Web3 operator: business development, exchange listings, community, content and "
      + "growth. Co-founded Dragon Farm; background in robotics AI retail and RWA "
      + "gamification.",
  },
  {
    name: "Antonio K.",
    role: "Chief Architect",
    based: "Korea",
    avatar: "/team/antonio.webp",
    motif: "Lattice and rotation arcs — transform geometry",
    body:
      "AI systems engineer and the architect of the Kvasir runtime. Patent-pending edge "
      + "AI accelerator built on rotation-space alignment under orthogonal transforms. "
      + "Also the blockchain side: on-chain settlement, the token, and wallet "
      + "infrastructure.",
  },
  {
    name: "Louis E. Vandan",
    role: "AI Model Engineering",
    based: "Korea",
    avatar: "/team/louis.webp",
    motif: "A grid stepping down — quantisation",
    body:
      "GPU and NPU optimisation, model architecture analysis, quantisation. Responsible "
      + "for verification, tuning and serving optimisation across the swarm — including "
      + "the cross-backend equivalence checks that say a phone and an MI250 computed the "
      + "same thing.",
  },
];

function PersonCard({ person }: { person: Person }) {
  return (
    <Card className="flex flex-col gap-5 p-6 sm:flex-row sm:items-start">
      <img
        src={person.avatar}
        alt={`Illustrated avatar for ${person.name}. ${person.motif}.`}
        width={96}
        height={96}
        loading="lazy"
        className="h-24 w-24 shrink-0 rounded-2xl border border-line object-cover"
      />
      <div className="min-w-0">
        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <h3 className="text-lg font-semibold text-ink">{person.name}</h3>
          <span className="text-sm font-medium text-brand-300">{person.role}</span>
        </div>
        <div className="mt-1 text-xs uppercase tracking-wide text-ink-faint">{person.based}</div>
        <p className="mt-3 text-sm leading-relaxed text-ink-muted">{person.body}</p>
      </div>
    </Card>
  );
}

function StructureBox({
  title, tag, children,
}: { title: string; tag: string; children: ReactNode }) {
  return (
    <Card className="p-6">
      <div className="text-xs uppercase tracking-wide text-ink-faint">{tag}</div>
      <div className="mt-1 text-lg font-semibold text-ink">{title}</div>
      <div className="mt-3 space-y-2 text-sm leading-relaxed text-ink-muted">{children}</div>
    </Card>
  );
}

export default function TeamPage() {
  useEffect(() => {
    document.title = "Team & structure · Kvasir";
  }, []);

  return (
    <>
      <Nav />
      <main className="pb-24 pt-28">
        <Container>
          <Pill>Team</Pill>
          <h1 className="mt-4 text-3xl font-semibold tracking-tight text-ink sm:text-4xl">
            Who builds this
          </h1>
          <p className="mt-4 max-w-2xl text-base leading-relaxed text-ink-muted">
            Five people across three countries. The portraits are drawn line engravings rather
            than photographs — a deliberate choice, not a placeholder, and each carries a motif
            from the work that person does.
          </p>

          <div className="mt-10 grid gap-5">
            {PEOPLE.map((person) => (
              <PersonCard key={person.name} person={person} />
            ))}
          </div>

          {/* ---- corporate structure ---------------------------------- */}
          <section className="mt-20">
            <h2 className="text-2xl font-semibold tracking-tight text-ink">
              How the company is arranged
            </h2>
            <p className="mt-3 max-w-2xl text-sm leading-relaxed text-ink-muted">
              Issuance and engineering sit in different jurisdictions, on purpose. Korea runs one
              of the world's largest crypto markets and also prohibits token issuance from within
              it; the UAE has a virtual-asset regime that says plainly what an issuer may do. So
              the token is issued where that is settled law, and the engineering stays where the
              engineers are.
            </p>

            <div className="mt-8 grid gap-5 md:grid-cols-2">
              <StructureBox tag="Issuance · governance" title="UAE entity — Abu Dhabi">
                <p>
                  Anchored in ADGM, with Hub71 as the intended ecosystem home. Issues KVR, holds
                  governance, and is the entity that carries the regulatory relationship.
                </p>
                <p>
                  The compliance path runs through VARA: a legal opinion, alignment of the Abu
                  Dhabi entity to its requirements, and a VARA-licensed market maker before any
                  listing.
                </p>
              </StructureBox>

              <StructureBox tag="Engineering · R&D" title="Korea">
                <p>
                  The p4 engine, the bridge, the settlement gateway, the wallets, and the MI250
                  fleet the network serves from today. No token issuance happens here.
                </p>
                <p>
                  Business development and marketing leadership sit in the UAE alongside the
                  issuing entity, which is also where exchange relationships are held.
                </p>
              </StructureBox>
            </div>

            <Card className="mt-6 p-5">
              <p className="text-sm leading-relaxed text-ink-muted">
                <strong className="text-ink">What this separation buys.</strong> An exchange
                listing team asks two questions early: who issues the token, and under which
                regulator. A structure that answers both without a diagram of holding companies
                is worth more than it costs to set up. It also means a Korean engineering team can
                keep building without the issuance question hanging over the work.
              </p>
            </Card>
          </section>

          {/* ---- what we are honest about ------------------------------ */}
          <section className="mt-16">
            <h2 className="text-2xl font-semibold tracking-tight text-ink">
              Where the project actually stands
            </h2>
            <Card className="mt-5 p-6">
              <p className="text-sm leading-relaxed text-ink-muted">
                The engine and its settlement run today, on our own fleet. A device can register,
                contribute and be paid. What is not open yet is the serving ring itself — every
                model currently served runs on hardware we operate. Expert-grain sharding, the
                thing that lets a small device hold a slice of a model rather than a whole layer,
                is half carried onto the current engine: the market and the relays a phone needs
                are live, the shard download and the engine-side dispatch are not written. The{" "}
                <a className="text-brand-300 hover:underline" href="/technology">
                  engineering blog
                </a>{" "}
                says which is which, including the parts we got wrong on the way.
              </p>
            </Card>
          </section>

          <p className="mt-16 text-xs text-ink-muted">Last updated {UPDATED}.</p>
        </Container>
      </main>
      <Footer />
    </>
  );
}

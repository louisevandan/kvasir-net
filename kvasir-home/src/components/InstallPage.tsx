import { useEffect } from "react";
import type { ReactNode } from "react";
import { Container, Card, Pill } from "./ui";
import Nav from "./Nav";
import Footer from "./Footer";
import { DOWNLOADS } from "../content";

/* ==========================================================================
   Installing the mobile builds (/install).

   These are development builds, and both platforms treat a development build
   as something the owner of the device has to allow on purpose. That is a
   feature, not an obstacle — but it means the download alone is not enough,
   and a person who taps it and gets "app not installed" has no way to know
   why. This page is the missing half of the download.

   English only, like /legal: one authoritative text, no translation drift in
   instructions where a wrong word costs someone an hour.
   ========================================================================== */

const UPDATED = "2026-09-21";

function Step({ n, title, children }: { n: number; title: string; children: ReactNode }) {
  return (
    <li className="relative pl-12 pb-8 last:pb-0">
      <span
        className="absolute left-0 top-0 flex h-8 w-8 items-center justify-center rounded-full
                   border border-line bg-surface text-sm font-semibold text-ink"
      >
        {n}
      </span>
      <div className="text-sm font-semibold text-ink">{title}</div>
      <div className="mt-2 space-y-2 text-sm leading-relaxed text-ink-muted">{children}</div>
    </li>
  );
}

function Mono({ children }: { children: ReactNode }) {
  return (
    <code className="rounded bg-surface-2 px-1.5 py-0.5 font-mono text-[0.85em] text-ink">
      {children}
    </code>
  );
}

function Block({ children }: { children: string }) {
  return (
    <pre className="mt-2 overflow-x-auto rounded-lg border border-line bg-surface-2 p-3
                    font-mono text-xs leading-relaxed text-ink">
      {children}
    </pre>
  );
}

export default function InstallPage() {
  useEffect(() => {
    document.title = "Installing the mobile builds · Kvasir";
  }, []);

  return (
    <>
      <Nav />
      <main className="pb-24 pt-28">
        <Container>
          <Pill>Mobile</Pill>
          <h1 className="mt-4 text-3xl font-semibold tracking-tight text-ink sm:text-4xl">
            Installing the mobile builds
          </h1>
          <p className="mt-4 max-w-2xl text-base leading-relaxed text-ink-muted">
            The Kvasir wallet is not in the App Store or on Google Play yet. Both mobile builds
            are development builds, and both platforms require the owner of a device to allow
            one deliberately. Everything below is that permission, step by step.
          </p>

          <Card className="mt-8 p-5">
            <p className="text-sm leading-relaxed text-ink-muted">
              <strong className="text-ink">What a development build means here.</strong>{" "}
              The Android package is signed with a development key, not a release key. It
              installs and runs, but Android has no way to tell it apart from any other build
              signed with the same key, so treat it as a test build: keep it on a device you
              control, and expect to uninstall rather than upgrade when a release-signed build
              replaces it. The iOS build is signed for a specific device list and expires; it is
              not distributed through a store and cannot be installed by a link alone.
            </p>
          </Card>

          {/* ---- Android ------------------------------------------------ */}
          <section className="mt-14">
            <div className="flex flex-wrap items-baseline justify-between gap-3">
              <h2 className="text-2xl font-semibold tracking-tight text-ink">Android</h2>
              <a
                href={DOWNLOADS.android}
                className="rounded-lg border border-line px-4 py-2 text-sm font-semibold text-ink
                           transition hover:border-ink-muted"
              >
                Download the APK
              </a>
            </div>
            <p className="mt-3 max-w-2xl text-sm leading-relaxed text-ink-muted">
              Requires Android 8.0 or later on a 64-bit ARM device — which is every phone sold in
              the last several years. About 27 MB.
            </p>

            <ol className="mt-8 border-l border-line pl-0">
              <Step n={1} title="Download it on the phone itself">
                <p>
                  Open this page on the phone and tap <em>Download the APK</em>. Downloading on a
                  computer and copying the file across also works, but it is the longer road.
                </p>
              </Step>
              <Step n={2} title="Allow your browser to install apps">
                <p>
                  Android blocks installs from anywhere but a store until you permit a specific
                  app to ask. When you open the downloaded file you will be told the browser is
                  not allowed to install unknown apps; follow the prompt to{" "}
                  <Mono>Settings → Install unknown apps</Mono> and turn it on for that browser
                  only.
                </p>
                <p>
                  On Samsung devices the same setting is under{" "}
                  <Mono>Settings → Apps → Special access → Install unknown apps</Mono>. Turn it
                  back off afterwards if you prefer; the installed app keeps working.
                </p>
              </Step>
              <Step n={3} title="Install, and read the scanner's warning">
                <p>
                  Play Protect will say the app was not scanned or comes from an unknown
                  developer. That is exactly what it should say about a build signed with a
                  development key. Choose <em>Install anyway</em> only because you know where
                  this file came from.
                </p>
              </Step>
              <Step n={4} title="If it says “App not installed”">
                <p>
                  Almost always one of two things. Either an older Kvasir build signed with a
                  different key is already on the device — uninstall it first, and note that
                  uninstalling removes its wallet, so export your recovery phrase beforehand. Or
                  the download was truncated: check the file is about 27 MB and download again.
                </p>
              </Step>
              <Step n={5} title="Verify what you installed (optional)">
                <p>
                  If you want to be sure the file is the one we published, compare its SHA-256
                  against the value on this page.
                </p>
                <Block>{`shasum -a 256 Kvasir-Wallet-android-arm64.apk`}</Block>
              </Step>
            </ol>
          </section>

          {/* ---- iOS ---------------------------------------------------- */}
          <section className="mt-16">
            <h2 className="text-2xl font-semibold tracking-tight text-ink">iOS</h2>
            <p className="mt-3 max-w-2xl text-sm leading-relaxed text-ink-muted">
              iOS has no equivalent of the Android download: Apple will not run an application on
              a device unless the application is signed for that specific device, so there is no
              file we can host that you could install by tapping it. Building it yourself takes
              about twenty minutes, most of which is Xcode downloading things.
            </p>

            <Card className="mt-6 p-5">
              <p className="text-sm leading-relaxed text-ink-muted">
                <strong className="text-ink">You will need</strong> a Mac with Xcode 16 or later,
                a USB cable, and an Apple ID. A paid Apple Developer account is not required — a
                free one works, with two limits worth knowing before you start: the app stops
                launching after <strong className="text-ink">seven days</strong> and has to be
                re-installed from Xcode, and a device may hold at most{" "}
                <strong className="text-ink">three</strong> apps signed with a free account.
              </p>
            </Card>

            <ol className="mt-8 border-l border-line pl-0">
              <Step n={1} title="Get the source and its build tooling">
                <Block>{`git clone https://github.com/louisevandan/kvasir-net.git
cd kvasir-net
brew install xcodegen`}</Block>
              </Step>
              <Step n={2} title="Build the native libraries the app links">
                <p>
                  The wallet runs inference on the device itself, so it links a compiled ring
                  stage and expert worker. This produces them; it is the slow step.
                </p>
                <Block>{`git submodule update --init --recursive
./scripts/build-ios-ring.sh`}</Block>
              </Step>
              <Step n={3} title="Generate the Xcode project">
                <p>
                  The project file is generated from <Mono>wallet/ios/project.yml</Mono> rather
                  than committed, so this step is required and is not optional tidiness.
                </p>
                <Block>{`cd wallet/ios
xcodegen generate
open KvasirWallet.xcodeproj`}</Block>
              </Step>
              <Step n={4} title="Sign it with your own Apple ID">
                <p>
                  In Xcode, select the <Mono>KvasirWallet</Mono> target, open{" "}
                  <em>Signing &amp; Capabilities</em>, tick <em>Automatically manage signing</em>,
                  and choose your own team. Add your Apple ID under{" "}
                  <Mono>Xcode → Settings → Accounts</Mono> if it is not listed.
                </p>
                <p>
                  Change the bundle identifier to something of your own — append your initials,
                  for instance. A free account cannot claim an identifier someone else has
                  already registered, and{" "}
                  <Mono>ai.banya.linkcpp.wallet</Mono> is registered to us.
                </p>
              </Step>
              <Step n={5} title="Connect the phone and run">
                <p>
                  Plug the phone in, unlock it, and tap <em>Trust</em> when it asks about this
                  computer. Pick it as the run destination in Xcode and press Run.
                </p>
              </Step>
              <Step n={6} title="Trust the developer on the phone">
                <p>
                  The first launch will be refused with “Untrusted Developer”. Go to{" "}
                  <Mono>Settings → General → VPN &amp; Device Management</Mono>, tap your Apple
                  ID, and trust it. Then launch the app from the home screen.
                </p>
              </Step>
            </ol>

            <Card className="mt-8 p-5">
              <p className="text-sm leading-relaxed text-ink-muted">
                <strong className="text-ink">If the install fails</strong> with “maximum number of
                apps using a free developer profile”, a free account allows three — delete one of
                the others from the device. If the build fails at the link step, step 2 did not
                finish; run it again and read its output rather than Xcode's.
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

#!/bin/sh
# Install a Kvasir expert node on a headless Linux server.
#
#   curl -fsSL https://pub-3fa7c08233cd497dbd39f89a9093c965.r2.dev/node/install.sh | sh
#
# What it does, in order: work out the architecture, make sure there is a Node
# runtime new enough, fetch the release and check its hash, unpack it, create a
# wallet key if there is none, and write a systemd unit. It does not start the
# node: how much GPU memory to lend is a decision the operator has to make, and
# a default that guesses would either waste the card or take the machine down.
#
# It is deliberately POSIX sh, not bash — the smallest server images have dash
# as /bin/sh and nothing else.
#
# Nothing here needs root. Run it as root and it installs to /opt and writes a
# system unit; run it as a user and it installs under ~/.local and writes a
# user unit. The second is the better default on a shared box.
set -eu

BASE="${KVASIR_DOWNLOAD_BASE:-https://pub-3fa7c08233cd497dbd39f89a9093c965.r2.dev/node}"
NODE_MIN_MAJOR=20
NODE_VERSION="${KVASIR_NODE_VERSION:-v22.22.2}"

say() { printf '%s\n' "$*"; }
die() { printf 'install: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required and was not found"; }

# ---- what are we on -------------------------------------------------------

[ "$(uname -s)" = Linux ] || die "this installs a Linux node; for macOS or Windows use the desktop app"
need curl
need tar

case "$(uname -m)" in
  x86_64)  ARCH=x64 ;;
  aarch64|arm64) ARCH=arm64 ;;
  *) die "no build for $(uname -m) — x86_64 and aarch64 are what exist" ;;
esac

if [ "$(id -u)" = 0 ]; then
  PREFIX="${KVASIR_PREFIX:-/opt/kvasir-node}"
  UNIT_DIR=/etc/systemd/system
  SYSTEMCTL="systemctl"
else
  PREFIX="${KVASIR_PREFIX:-$HOME/.local/share/kvasir-node}"
  UNIT_DIR="$HOME/.config/systemd/user"
  SYSTEMCTL="systemctl --user"
fi
CONFIG_DIR="${KVASIR_CONFIG:-$HOME/.config/kvasir}"

say "Kvasir node installer"
say "  architecture  linux-$ARCH"
say "  install to    $PREFIX"
say ""

# ---- a Node runtime -------------------------------------------------------
#
# The node is JavaScript, so it needs one. Rather than send people to their
# distribution's packages — where "nodejs" is often several years old and
# upgrading it is a system-wide decision — this uses whatever is already there
# if it is new enough, and otherwise unpacks an official build inside the
# install directory, touching nothing else on the machine.

RUNTIME=""
if command -v node >/dev/null 2>&1; then
  have="$(node -p 'process.versions.node.split(".")[0]' 2>/dev/null || echo 0)"
  if [ "$have" -ge "$NODE_MIN_MAJOR" ] 2>/dev/null; then
    RUNTIME="$(command -v node)"
    say "using the Node already installed ($(node -v))"
  else
    say "Node $(node -v) is older than $NODE_MIN_MAJOR; fetching a private one"
  fi
fi

mkdir -p "$PREFIX"

if [ -z "$RUNTIME" ]; then
  if [ -x "$PREFIX/runtime/bin/node" ]; then
    RUNTIME="$PREFIX/runtime/bin/node"
    say "using the Node from a previous install"
  else
    case "$ARCH" in x64) NARCH=x64 ;; arm64) NARCH=arm64 ;; esac
    url="https://nodejs.org/dist/$NODE_VERSION/node-$NODE_VERSION-linux-$NARCH.tar.xz"
    say "downloading Node $NODE_VERSION for linux-$NARCH"
    rm -rf "$PREFIX/runtime" && mkdir -p "$PREFIX/runtime"
    curl -fsSL "$url" | tar -xJ -C "$PREFIX/runtime" --strip-components=1 \
      || die "could not unpack Node from $url"
    RUNTIME="$PREFIX/runtime/bin/node"
  fi
fi

# ---- the release ----------------------------------------------------------
#
# latest.json names the current build and its SHA-256. The hash is checked
# before anything is unpacked: this script is piped from the network into a
# shell, which is only as safe as what it goes on to run.

say "fetching the release index"
index="$(curl -fsSL "$BASE/latest.json")" || die "could not reach $BASE/latest.json"
pick() { printf '%s' "$index" | tr ',' '\n' | grep "\"$1\"" | head -1 | sed 's/.*: *"//; s/".*//'; }
VERSION="$(pick version)"
TARBALL="$(pick "linux-$ARCH")"
SHA="$(pick "linux-$ARCH-sha256")"
[ -n "$TARBALL" ] || die "the index has no build for linux-$ARCH yet"
[ -n "$SHA" ] || die "the index has no checksum for linux-$ARCH; refusing to install unverified"

say "  version       $VERSION"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
say "downloading $TARBALL"
curl -fsSL "$BASE/$TARBALL" -o "$tmp/pkg.tar.gz" || die "download failed"

if command -v sha256sum >/dev/null 2>&1; then
  got="$(sha256sum "$tmp/pkg.tar.gz" | cut -d' ' -f1)"
elif command -v shasum >/dev/null 2>&1; then
  got="$(shasum -a 256 "$tmp/pkg.tar.gz" | cut -d' ' -f1)"
else
  die "no sha256sum or shasum to verify the download with"
fi
[ "$got" = "$SHA" ] || die "checksum mismatch: expected $SHA, got $got"
say "  checksum      ok"

tar -xzf "$tmp/pkg.tar.gz" -C "$PREFIX" --strip-components=1
chmod +x "$PREFIX/kvasir-node.cjs" 2>/dev/null || true
[ -f "$PREFIX/p4/p4-agent" ] && chmod +x "$PREFIX/p4/p4-agent"

# A launcher that knows which Node to use.
#
# kvasir-node.cjs starts with `#!/usr/bin/env node`, which fails outright on a
# server with no node on PATH — and that is the common case here, since this
# script may have just unpacked a private runtime precisely because there was
# none. Every instruction printed below would then be a command that does not
# run. So the launcher pins the interpreter this install chose.
cat > "$PREFIX/kvasir-node" <<LAUNCH
#!/bin/sh
exec "$RUNTIME" "$PREFIX/kvasir-node.cjs" "\$@"
LAUNCH
chmod +x "$PREFIX/kvasir-node"

# ---- a key ----------------------------------------------------------------
#
# The wallet this node earns into. Generated here rather than asked for,
# because the alternative is people pasting a funded wallet's secret onto a
# server. Nothing needs to be in it: there is no stake and no minimum balance.

KEY="$CONFIG_DIR/node-key.json"
mkdir -p "$CONFIG_DIR"
chmod 700 "$CONFIG_DIR" 2>/dev/null || true
if [ -f "$KEY" ]; then
  say "keeping the existing key at $KEY"
else
  "$PREFIX/kvasir-node" keygen --out "$KEY" >/dev/null
  say "created a node key at $KEY"
fi
ADDRESS="$("$PREFIX/kvasir-node" address --key "$KEY" 2>/dev/null || true)"

# ---- the unit -------------------------------------------------------------
#
# Written, not enabled. --budget is the one thing nobody else can decide: it is
# how much GPU memory this machine lends, and the number that decides whether
# the node is a good neighbour on a box that has other work to do.

mkdir -p "$UNIT_DIR"
cat > "$UNIT_DIR/kvasir-node.service" <<UNIT
[Unit]
Description=Kvasir expert node
After=network-online.target
Wants=network-online.target

[Service]
# --budget is in GiB and has no default here on purpose. Set it to the GPU
# memory you are willing to lend, leaving room for anything else on this card.
ExecStart=$PREFIX/kvasir-node run --key $KEY --budget REPLACE_ME
Restart=on-failure
RestartSec=10
# The node holds a wallet key and downloads model weights; it needs nothing
# else on the machine.
NoNewPrivileges=yes
PrivateTmp=yes

[Install]
WantedBy=default.target
UNIT

say ""
say "installed."
[ -n "$ADDRESS" ] && say "  wallet        $ADDRESS"
say "  node          $PREFIX/kvasir-node"
say "  key           $KEY   (mode 600 — back it up; rewards are paid to it)"
say "  unit          $UNIT_DIR/kvasir-node.service"
say ""
say "Two things left, and only you can do the first:"
say ""
say "  1. Say how much GPU memory to lend. Edit the unit and replace REPLACE_ME"
say "     with a number of GiB, e.g. 8."
say ""
say "  2. Get the expert worker for this machine. The node needs a compute"
say "     binary to serve with; it is a separate download because it is large"
say "     and specific to your GPU:"
say ""
say "       $PREFIX/kvasir-node worker --install"
say ""
say "Then:"
say "       $SYSTEMCTL daemon-reload && $SYSTEMCTL enable --now kvasir-node"
say "       $SYSTEMCTL status kvasir-node"
say ""
say "Watch what it earns at https://gate.kvasir-ai.net/api/node/status/${ADDRESS:-<address>}"

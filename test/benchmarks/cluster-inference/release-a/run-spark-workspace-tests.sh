#!/usr/bin/env bash
# Fixed, low-load Rust verification for the Release A Spark checkout.
set -euo pipefail

if (( $# != 2 )); then
    printf 'usage: %s EXPECTED_COMMIT TARGET_DIR\n' "$0" >&2
    exit 2
fi

expected_commit=$1
target_dir=$2
source_dir=$(git rev-parse --show-toplevel)
cargo=/home/m42/.cargo/bin/cargo
node_dir=/home/m42/.nvm/versions/node/v24.19.0/bin
python=/usr/bin/python3

test "$(git -C "$source_dir" rev-parse HEAD)" = "$expected_commit"
test -z "$(git -C "$source_dir" status --porcelain)"
test -x "$cargo"
test -x "$node_dir/node"
test -x "$python"
test "$(nproc)" -ge 20
test "$("$cargo" --version)" = 'cargo 1.97.1 (c980f4866 2026-06-30)'
test "$("$node_dir/node" --version)" = 'v24.19.0'
test "$("$python" --version)" = 'Python 3.12.3'

jobs=$(( $(nproc) * 7 / 10 ))
test "$jobs" -ge 1
export PATH="$node_dir:/home/m42/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
export HF_TEST_PYTHON="$python"
export RUST_TEST_THREADS="$jobs"

nice -n 10 "$cargo" test --workspace --no-fail-fast --locked \
    -j "$jobs" --target-dir "$target_dir"

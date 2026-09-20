#!/usr/bin/env bash
# Copy the shared wallet spec into the two mobile bundles.
#
# `wallet/shared-spec/` is the source of truth; each app bundles its own copy
# because neither build system can reach across the tree at run time. Those
# copies drift silently — both were missing `nodeGuideUrl` for as long as it had
# existed — and nothing catches it, because a missing key reads as a nil default
# rather than an error. SharedSpec.swift has named this script since before it
# was written; run it whenever shared-spec changes, and in CI with --check.
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
targets=(
  "$here/android/core/src/main/resources"
  "$here/ios/Core/Sources/Core/Resources"
)
files=(wallet-constants.json token.devnet.json)
check=${1:-}
status=0
for target in "${targets[@]}"; do
  for file in "${files[@]}"; do
    src="$here/shared-spec/$file"
    [ -f "$src" ] || { echo "missing source: $src" >&2; exit 1; }
    if [ "$check" = "--check" ]; then
      if ! diff -q "$src" "$target/$file" > /dev/null 2>&1; then
        echo "out of date: $target/$file"
        diff -u "$src" "$target/$file" || true
        status=1
      fi
    else
      cp "$src" "$target/$file"
      echo "synced $target/$file"
    fi
  done
done
exit $status

#!/usr/bin/env bash
# Measure `akron scan --json` wall time and output hash on a corpus, for the
# TKI-58 perf round. Runs N times, reports min/median wall time, and the
# sha256 of the JSON output — the correctness oracle: this hash must stay
# byte-identical across every optimization in the round.
#
# Uses hyperfine when it's on PATH (better statistics); otherwise falls back
# to a plain loop timed with /usr/bin/time.
#
# Usage:
#   ./scripts/perf-pipeline.sh <corpus-path> [runs]
#
# Does not build for you — run `cargo build --release` first so the binary
# under test is the one you intend to measure.
set -euo pipefail
cd "$(dirname "$0")/.."

corpus="${1:?usage: perf-pipeline.sh <corpus-path> [runs]}"
runs="${2:-5}"
bin="target/release/akron"

if [ ! -x "$bin" ]; then
  echo "error: $bin not found — run 'cargo build --release' first" >&2
  exit 1
fi

out="$(mktemp)"
trap 'rm -f "$out"' EXIT

echo "corpus: $corpus"
echo "runs:   $runs"

if command -v hyperfine >/dev/null 2>&1; then
  hyperfine --warmup 1 --min-runs "$runs" \
    "$bin scan $corpus --json $out"
else
  times=()
  for i in $(seq 1 "$runs"); do
    t=$( { /usr/bin/time -p "$bin" scan "$corpus" --json "$out" >/dev/null; } 2>&1 | awk '/^real/ {print $2}' )
    times+=("$t")
  done
  sorted=($(printf '%s\n' "${times[@]}" | sort -n))
  mid=$(( runs / 2 ))
  echo "times:  ${times[*]}"
  echo "min:    ${sorted[0]}s"
  echo "median: ${sorted[$mid]}s"
fi

sha=$(shasum -a 256 "$out" | awk '{print $1}')
echo "sha256: $sha"

#!/usr/bin/env bash
# Reproduce TKI-59's wall-time/latency measurements for explore's compute:
# boot wall time, cold /api/symbols latency, and /api/sublayout latency
# (min/median over N samples) for a drill path. Also prints sha256 of the
# key /api/* response bodies — the correctness oracle for any layout.rs/
# pca.rs/explain.rs change (every byte must match across runs).
#
# Usage:
#   ./scripts/perf-layout.sh [corpus-dir] [drill-path] [sublayout-samples]
#
# Defaults match the TKI-59 scale target: scrapy-full's top-level "scrapy"
# dir (~1700 members), 10 sublayout samples.
set -euo pipefail
cd "$(dirname "$0")/.."

corpus="${1:-/tmp/akron-corpora/scrapy-full}"
drill="${2:-scrapy}"
samples="${3:-10}"

if [ ! -d "$corpus" ]; then
  echo "error: corpus dir not found: $corpus" >&2
  exit 1
fi

echo "==> building release"
cargo build --release --quiet

bin=./target/release/akron
outdir=$(mktemp -d)
trap 'rm -rf "$outdir"' EXIT

wait_for_boot() {
  local logfile="$1"
  local timeout_s="${2:-120}"
  local waited=0
  while (( waited < timeout_s * 10 )); do
    if grep -q "explore —" "$logfile" 2>/dev/null; then
      return 0
    fi
    sleep 0.1
    waited=$((waited + 1))
  done
  echo "error: server did not report boot within ${timeout_s}s" >&2
  return 1
}

port_from_log() {
  grep -oE ':[0-9]+$' "$1" | tr -d ':'
}

kill_server() {
  local pid="$1"
  kill "$pid" >/dev/null 2>&1 || true
  wait "$pid" 2>/dev/null || true
}

echo "==> warming embed cache (unmeasured)"
warmlog="$outdir/warm.log"
"$bin" explore "$corpus" --port 0 > "$warmlog" 2>&1 &
warmpid=$!
wait_for_boot "$warmlog"
kill_server "$warmpid"

echo "==> measuring boot wall time (5 runs, cache warm)"
boot_times=()
for i in $(seq 1 5); do
  bootlog="$outdir/boot_$i.log"
  start=$(python3 -c 'import time; print(time.time())')
  "$bin" explore "$corpus" --port 0 > "$bootlog" 2>&1 &
  pid=$!
  wait_for_boot "$bootlog"
  end=$(python3 -c 'import time; print(time.time())')
  dt=$(python3 -c "print(f'{$end - $start:.4f}')")
  boot_times+=("$dt")
  echo "  run $i: ${dt}s  ($(cat "$bootlog"))"
  if [ "$i" -eq 5 ]; then
    # keep the last server up for the latency measurements below
    server_pid=$pid
    server_log=$bootlog
  else
    kill_server "$pid"
  fi
done
python3 -c "
times = [$(IFS=,; echo "${boot_times[*]}")]
times.sort()
print(f'boot: min={times[0]:.4f}s median={times[len(times)//2]:.4f}s max={times[-1]:.4f}s')
"

port=$(port_from_log "$server_log")

echo "==> cold /api/symbols latency"
start=$(python3 -c 'import time; print(time.time())')
curl -s "http://127.0.0.1:$port/api/symbols" -o "$outdir/symbols.json"
end=$(python3 -c 'import time; print(time.time())')
python3 -c "print(f'  {$end - $start:.4f}s')"

curl -s "http://127.0.0.1:$port/api/meta" -o "$outdir/meta.json"

echo "==> /api/sublayout?path=$drill latency ($samples samples)"
sub_times=()
for i in $(seq 1 "$samples"); do
  start=$(python3 -c 'import time; print(time.time())')
  curl -s "http://127.0.0.1:$port/api/sublayout?path=$drill" -o "$outdir/sublayout_$i.json"
  end=$(python3 -c 'import time; print(time.time())')
  dt=$(python3 -c "print(f'{$end - $start:.4f}')")
  sub_times+=("$dt")
done
python3 -c "
times = [$(IFS=,; echo "${sub_times[*]}")]
times.sort()
print(f'  min={times[0]:.4f}s median={times[len(times)//2]:.4f}s max={times[-1]:.4f}s')
"

echo "==> sha256 (correctness oracle)"
shasum -a 256 "$outdir/symbols.json" "$outdir/meta.json" "$outdir/sublayout_1.json"

echo "==> repeated-sublayout sha identity check"
if shasum -a 256 "$outdir"/sublayout_*.json | awk '{print $1}' | sort -u | wc -l | grep -q '^ *1$'; then
  echo "  OK: all $samples sublayout responses sha-identical"
else
  echo "  FAIL: sublayout responses differ across repeats" >&2
  kill_server "$server_pid"
  exit 1
fi

kill_server "$server_pid"
echo "==> done"

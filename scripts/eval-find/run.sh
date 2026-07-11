#!/usr/bin/env bash
# TKI-64 graded `akron find` eval: P@5 over scripts/eval-find/questions.json
# against public corpora (httpx-full, scrapy-full). Never touches private
# corpora — corpus roots are passed in explicitly.
#
# Usage:
#   scripts/eval-find/run.sh <akron-binary> <corpora-base-dir> [top]
#
# Example:
#   scripts/eval-find/run.sh ./target/release/akron /tmp/akron-corpora 5
#
# Prints one line per question ("<id>  P@5=x/5  <matched qnames>") then a
# per-corpus and overall summary. Exits non-zero only on a hard failure
# (missing binary/corpus/jq) — a P@5 of 0 on some question is a valid,
# reportable result, not a script error.

set -euo pipefail

BIN="${1:?usage: run.sh <akron-binary> <corpora-base-dir> [top]}"
CORPORA_DIR="${2:?usage: run.sh <akron-binary> <corpora-base-dir> [top]}"
TOP="${3:-5}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
QUESTIONS="$SCRIPT_DIR/questions.json"

command -v jq >/dev/null || { echo "run.sh: jq is required" >&2; exit 1; }
[ -x "$BIN" ] || { echo "run.sh: binary not found or not executable: $BIN" >&2; exit 1; }
[ -f "$QUESTIONS" ] || { echo "run.sh: missing $QUESTIONS" >&2; exit 1; }

n=$(jq '.questions | length' "$QUESTIONS")

total_hits=0
total_possible=0
# bash 3.2 (macOS default) has no associative arrays — accumulate per-corpus
# totals in a scratch file instead ("<corpus> <hits> <possible>" lines,
# summed at the end).
corpus_totals=$(mktemp)
trap 'rm -f "$corpus_totals"' EXIT

for i in $(seq 0 $((n - 1))); do
    id=$(jq -r ".questions[$i].id" "$QUESTIONS")
    corpus=$(jq -r ".questions[$i].corpus" "$QUESTIONS")
    query=$(jq -r ".questions[$i].query" "$QUESTIONS")
    root="$CORPORA_DIR/$corpus"
    [ -d "$root" ] || { echo "run.sh: corpus dir missing: $root" >&2; exit 1; }

    out=$("$BIN" find "$root" "$query" --top "$TOP" --json 2>/dev/null)
    hits_json=$(echo "$out" | jq -c '.hits')

    matched=0
    matched_names=""
    hit_count=$(echo "$hits_json" | jq 'length')
    for h in $(seq 0 $((hit_count - 1))); do
        qname=$(echo "$hits_json" | jq -r ".[$h].qname")
        is_match=$(jq --arg q "$qname" ".questions[$i].expected | any(. as \$e | \$q | contains(\$e))" "$QUESTIONS")
        if [ "$is_match" = "true" ]; then
            matched=$((matched + 1))
            matched_names="$matched_names $qname"
        fi
    done

    echo "$id  P@${TOP}=${matched}/${TOP}  [$corpus]  ${query}  ->${matched_names}"

    total_hits=$((total_hits + matched))
    total_possible=$((total_possible + TOP))
    echo "$corpus $matched $TOP" >>"$corpus_totals"
done

echo "---"
for c in $(cut -d' ' -f1 "$corpus_totals" | sort -u); do
    h=$(awk -v c="$c" '$1==c{s+=$2} END{print s+0}' "$corpus_totals")
    p=$(awk -v c="$c" '$1==c{s+=$3} END{print s+0}' "$corpus_totals")
    printf "%-14s P@%d = %d/%d = %.3f\n" "$c" "$TOP" "$h" "$p" "$(echo "scale=3; $h/$p" | bc)"
done
printf "%-14s P@%d = %d/%d = %.3f\n" "overall" "$TOP" "$total_hits" "$total_possible" "$(echo "scale=3; $total_hits/$total_possible" | bc)"

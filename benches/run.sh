#!/usr/bin/env bash
# Runs the roaring128 benchmark suite with the settings used for the reported numbers.
#
#   benches/run.sh                       full suite: ops + setops (criterion), memory table, report
#   benches/run.sh baselines             only the baselines (works while roaring128 is todo!())
#   benches/run.sh ops|setops [ARGS...]  one criterion binary; ARGS go to criterion (e.g. a regex
#                                        filter such as 'insert/.*/uuid_v4' or '/roaring128/')
#   benches/run.sh memory [WORKLOAD...]  memory table only (optional workload substrings)
#   benches/run.sh report                Markdown summary of target/criterion
#   benches/run.sh test                  run every benchmark once (no measurement) + self-check
#
# Settings (environment):
#   BENCH_WARMUP=0.3 BENCH_MEASURE=1 BENCH_SAMPLES=10   criterion warm-up / measurement seconds,
#                                                        samples (a full run takes ~10-15 min)
#   BENCH_CPU=<n>     pin to one core with taskset (less scheduler migration noise)
#   BENCH_OUT=<dir>   logs and report (default target/bench-results)
#   BENCH_SKIP_CHECK=1  skip the self-check (not recommended)
#
# On a laptop, frequency scaling dominates the noise: plug in, close other work, keep the same
# power profile between runs. Every binary prints the SIMD backend, CPU, governor and load.
set -euo pipefail
cd "$(dirname "$0")/.."

WARMUP=${BENCH_WARMUP:-0.3}
MEASURE=${BENCH_MEASURE:-1}
SAMPLES=${BENCH_SAMPLES:-10}
OUT=${BENCH_OUT:-target/bench-results}
CRIT=(--warm-up-time "$WARMUP" --measurement-time "$MEASURE" --sample-size "$SAMPLES"
      --nresamples 20000 --noplot)
BASELINES='/(wrapped64|btreeset|hashset|treemap)/'
PIN=()
if [[ -n "${BENCH_CPU:-}" ]] && command -v taskset > /dev/null; then
    PIN=(taskset -c "$BENCH_CPU")
fi
mkdir -p "$OUT"

criterion() { # <bench> [criterion args...]
    local bench=$1
    shift
    "${PIN[@]}" cargo bench --all-features --bench "$bench" -- "${CRIT[@]}" "$@" 2>&1 \
        | tee "$OUT/$bench.log"
}

memory() {
    "${PIN[@]}" cargo bench --all-features --bench memory -- "$@" 2>&1 | grep -v '^\s*\(Compiling\|Finished\|Running\)' \
        | tee "$OUT/memory.md"
}

report() {
    python3 benches/report.py target/criterion | tee "$OUT/report.md"
}

cmd=${1:-all}
[[ $# -gt 0 ]] && shift
cargo bench --all-features --no-run --quiet
case $cmd in
    all)
        start=$(date +%s)
        criterion ops "$@"
        criterion setops "$@"
        memory
        report > /dev/null
        echo "done in $(( $(date +%s) - start ))s; results in $OUT/{ops.log,setops.log,memory.md,report.md}"
        ;;
    baselines)
        criterion ops "$BASELINES"
        criterion setops "$BASELINES"
        memory
        ;;
    ops | setops) criterion "$cmd" "$@" ;;
    memory) memory "$@" ;;
    report) report ;;
    test)
        cargo bench --all-features --bench ops -- --test "$@"
        cargo bench --all-features --bench setops -- --test "$@"
        ;;
    *)
        sed -n '2,24p' "$0"
        exit 2
        ;;
esac

#!/usr/bin/env bash
# Runs the roaring128 benchmark suite and writes a self-describing results directory.
#
#   benches/run.sh [all]                 ops + setops, every implementation in its own process (in a
#                                        shuffled order per run), memory table, metadata, report
#   benches/run.sh baselines             the same without roaring128 (the memory table still
#                                        measures it)
#   benches/run.sh ops|setops [ARGS...]  one criterion binary; ARGS go to criterion (a regex filter
#                                        such as 'insert/.*/uuid_v4')
#   benches/run.sh memory [WORKLOAD...]  memory table only (optional workload substrings)
#   benches/run.sh report [DIR] [OPTS]   Markdown summary of a results directory (default: latest);
#                                        OPTS go to report.py (e.g. --subject btreeset)
#   benches/run.sh test [ARGS...]        run every benchmark once (no measurement) + self-check
#   benches/run.sh meta                  print the run metadata
#
# all / baselines / ops / setops also take criterion ARGS (e.g. a filter).
#
# Settings (environment):
#   BENCH_WARMUP=1 BENCH_MEASURE=3 BENCH_SAMPLES=30
#                       criterion warm-up and measurement seconds and samples per benchmark; an
#                       operation slower than MEASURE/SAMPLES still gets SAMPLES single-iteration
#                       samples (criterion stretches the measurement time)
#   BENCH_REPEAT=1      run ops + setops this many times (fresh processes, new order each time);
#                       report.py then shows the run-to-run spread and widens the intervals
#   BENCH_IMPLS="a b"   implementations to run (default: all that apply): roaring128 wrapped64
#                       btreeset sortedvec hashset hashset_fx treemap
#   BENCH_SEED=<n>      seed of the order shuffle (default: random; recorded in meta.txt)
#   BENCH_CPU=<list>    optional: pin with `taskset -c <list>` (Linux only), e.g. BENCH_CPU=2
#   BENCH_OUT=<dir>     results directory (default: target/bench-results/<UTC timestamp>)
#   BENCH_SKIP_CHECK=1  skip the self-check (not recommended)
#
# Results directory: meta.txt (commit, toolchain, CPU, backend, settings, order), run-N/criterion
# (criterion data of run N), run-N/logs, processes.tsv (wall time and peak RSS of every process),
# memory.md and report.md. Needs bash, cargo, python3 and git; Linux (x86_64, aarch64) or macOS.
# benches/README.md explains the suite and how to read the report.
#
# For stable numbers: an otherwise idle machine, on AC power, a fixed CPU frequency policy
# (e.g. the `performance` governor) and the same settings between runs.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

WARMUP=${BENCH_WARMUP:-1}
MEASURE=${BENCH_MEASURE:-3}
SAMPLES=${BENCH_SAMPLES:-30}
REPEAT=${BENCH_REPEAT:-1}
SEED=${BENCH_SEED:-$((RANDOM * 32768 + RANDOM))}
FEATURES=(--all-features)
ALL_IMPLS="roaring128 wrapped64 btreeset sortedvec hashset hashset_fx treemap"
SETOPS_IMPLS="roaring128 wrapped64 btreeset sortedvec treemap"
RESULTS=target/bench-results

CRIT=(--warm-up-time "$WARMUP" --measurement-time "$MEASURE" --sample-size "$SAMPLES" --noplot)
PIN=()
if [[ -n "${BENCH_CPU:-}" ]]; then
    if command -v taskset > /dev/null; then
        PIN=(taskset -c "$BENCH_CPU")
    else
        echo "warning: BENCH_CPU is set but taskset is not available; not pinning" >&2
    fi
fi

# Runs a command and appends "<label>\t<wall s>\t<peak RSS MiB>\t<exit code>" to a TSV file
# (the peak RSS is that of the largest process in the command's tree).
PEAK_PY='import resource, subprocess, sys, time
start = time.time()
rc = subprocess.call(sys.argv[3:])
rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
mib = rss / 1048576 if sys.platform == "darwin" else rss / 1024
with open(sys.argv[1], "a") as f:
    f.write("%s\t%.1f\t%.0f\t%d\n" % (sys.argv[2], time.time() - start, mib, rc))
sys.exit(rc)'

# ---------------------------------------------------------------------------------------------
# Metadata
# ---------------------------------------------------------------------------------------------

first_line() { # <file> <regex>: the value after ':' of the first matching line
    grep -m1 -E "$2" "$1" 2> /dev/null | cut -d: -f2- | sed 's/^[[:space:]]*//' || true
}

cpu_model() {
    local m=""
    if [[ -r /proc/cpuinfo ]]; then
        m=$(first_line /proc/cpuinfo '^model name')
        if [[ -z "$m" ]] && command -v lscpu > /dev/null; then
            m=$(lscpu 2> /dev/null | grep -m1 -E '^Model name' | cut -d: -f2- | sed 's/^[[:space:]]*//' || true)
            local vendor
            vendor=$(lscpu 2> /dev/null | grep -m1 -E '^Vendor ID' | cut -d: -f2- | sed 's/^[[:space:]]*//' || true)
            [[ -n "$m" && -n "$vendor" ]] && m="$vendor $m"
        fi
        if [[ -z "$m" ]]; then
            local impl part
            impl=$(first_line /proc/cpuinfo '^CPU implementer')
            part=$(first_line /proc/cpuinfo '^CPU part')
            [[ -n "$part" ]] && m="implementer $impl part $part"
        fi
    elif command -v sysctl > /dev/null; then
        m=$(sysctl -n machdep.cpu.brand_string 2> /dev/null || true)
    fi
    echo "${m:-unknown}"
}

# SIMD-relevant CPU flags: x86 `flags`, ARM `Features` (Linux) or hw.optional.* (macOS).
cpu_flags() {
    local f=""
    if [[ -r /proc/cpuinfo ]]; then
        f=$(first_line /proc/cpuinfo '^(flags|Features)[[:space:]]*:')
    elif command -v sysctl > /dev/null; then
        f=$(sysctl -a 2> /dev/null | grep -E '^hw\.optional\.[A-Za-z0-9_.]+: 1$' \
            | sed -E 's/^hw\.optional\.(arm\.)?//; s/: 1$//' | tr '\n' ' ' || true)
    fi
    local out
    out=$(echo "$f" | tr ' ' '\n' \
        | grep -E '^(sse4_2|popcnt|avx|avx2|bmi1|bmi2|lzcnt|abm|fma|avx512.*|asimd.*|neon|sve.*|FEAT_(DotProd|SVE.*|BF16|I8MM|FHM|FP16)|AdvSIMD.*)$' \
        | sort -u | tr '\n' ' ' || true)
    echo "${out:-none found}"
}

read_or() { # <file> <default>
    if [[ -r "$1" ]]; then tr -d '\n' < "$1"; else echo "$2"; fi
}

load_avg() {
    if [[ -r /proc/loadavg ]]; then
        cut -d' ' -f1-3 /proc/loadavg
    else
        sysctl -n vm.loadavg 2> /dev/null | tr -d '{}' | sed 's/^ *//; s/ *$//' || uptime
    fi
}

memory_info() {
    if [[ -r /proc/meminfo ]]; then
        awk '/^MemTotal/ {t = $2} /^MemAvailable/ {a = $2}
             END {printf "%.1f GiB total, %.1f GiB available\n", t / 1048576, a / 1048576}' /proc/meminfo
    else
        local b
        b=$(sysctl -n hw.memsize 2> /dev/null || echo 0)
        awk -v b="$b" 'BEGIN {printf "%.1f GiB total\n", b / 1073741824}'
    fi
}

backend() {
    cargo bench -q "${FEATURES[@]}" --bench memory -- --probe 2> /dev/null | tail -n 1 || echo "unknown"
}

meta() { # [command words...]
    echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "command: benches/run.sh $*"
    if git rev-parse --git-dir > /dev/null 2>&1; then
        local changes
        changes=$(git status --porcelain --untracked-files=no)
        echo "commit: $(git rev-parse HEAD) (dirty: $([[ -n "$changes" ]] && echo yes || echo no))"
        echo "branch: $(git rev-parse --abbrev-ref HEAD)"
        [[ -n "$changes" ]] && echo "$changes" | sed 's/^/  uncommitted: /'
    else
        echo "commit: unknown (not a git checkout)"
    fi
    echo "rustc -vV:"
    rustc -vV | sed 's/^/  /'
    echo "cargo: $(cargo -V)"
    echo "RUSTFLAGS: ${RUSTFLAGS:-<unset>}"
    echo "os: $(uname -srm)"
    echo "cpu: $(cpu_model)"
    echo "cpus online: $(getconf _NPROCESSORS_ONLN 2> /dev/null || echo '?')"
    echo "cpu simd flags: $(cpu_flags)"
    echo "roaring128 backend: $(backend)"
    local cpufreq=/sys/devices/system/cpu/cpu0/cpufreq
    echo "governor: $(read_or $cpufreq/scaling_governor n/a)," \
        "epp: $(read_or $cpufreq/energy_performance_preference n/a)," \
        "boost: $(read_or /sys/devices/system/cpu/cpufreq/boost n/a)"
    echo "load average at start: $(load_avg)"
    echo "memory: $(memory_info)"
    echo "settings: warmup=${WARMUP}s measure=${MEASURE}s samples=$SAMPLES repeat=$REPEAT" \
        "seed=$SEED pin=${BENCH_CPU:-none} features=${FEATURES[*]} skip_check=${BENCH_SKIP_CHECK:-0}"
    echo "process per implementation: yes (BENCH_IMPL=<name> per process, order below)"
}

# ---------------------------------------------------------------------------------------------
# Running
# ---------------------------------------------------------------------------------------------

new_out() {
    OUT=${BENCH_OUT:-$RESULTS/$(date -u +%Y%m%dT%H%M%SZ)}
    mkdir -p "$OUT"
    OUT=$(cd "$OUT" && pwd)
    if [[ -z "${BENCH_OUT:-}" ]]; then
        ln -sfn "$(basename "$OUT")" "$RESULTS/latest"
    fi
}

# Sets IMPLS_FOR to the requested implementations (BENCH_IMPLS) that apply to <bench>.
impls_for() { # <bench> [exclude]
    local applicable=$ALL_IMPLS
    [[ $1 == setops ]] && applicable=$SETOPS_IMPLS
    local requested=${BENCH_IMPLS:-$applicable}
    requested=${requested//,/ }
    IMPLS_FOR=()
    local i
    for i in $applicable; do
        [[ " $requested " == *" $i "* && "$i" != "${2:-}" ]] && IMPLS_FOR+=("$i")
    done
    return 0
}

# Fisher-Yates shuffle of IMPLS_FOR in place, driven by $RANDOM (seeded once with BENCH_SEED).
shuffle_impls() {
    local n=${#IMPLS_FOR[@]} i j t
    for ((i = n - 1; i > 0; i--)); do
        j=$((RANDOM % (i + 1)))
        t=${IMPLS_FOR[i]}
        IMPLS_FOR[i]=${IMPLS_FOR[j]}
        IMPLS_FOR[j]=$t
    done
}

FAILED=()

run_one() { # <run> <bench> <impl> [criterion args...]
    local run=$1 bench=$2 impl=$3
    shift 3
    local dir=$OUT/run-$run
    mkdir -p "$dir/logs" "$dir/criterion"
    echo "== run $run/$REPEAT: $bench, $impl"
    if ! BENCH_IMPL=$impl CRITERION_HOME=$dir/criterion \
        python3 -c "$PEAK_PY" "$OUT/processes.tsv" "$run"$'\t'"$bench"$'\t'"$impl" \
        ${PIN[@]+"${PIN[@]}"} cargo bench "${FEATURES[@]}" --bench "$bench" -- "${CRIT[@]}" "$@" \
        > "$dir/logs/$bench-$impl.log" 2>&1; then
        FAILED+=("run $run $bench $impl (see $dir/logs/$bench-$impl.log)")
        echo "   FAILED: $(grep -m1 -E 'panicked|error' "$dir/logs/$bench-$impl.log" || true)"
    fi
    grep -E '^(self-check|implementations)' "$dir/logs/$bench-$impl.log" | sed 's/^/   /' || true
}

suite() { # <exclude impl> [criterion args...]
    local exclude=$1
    shift
    local run bench impl
    for ((run = 1; run <= REPEAT; run++)); do
        for bench in ops setops; do
            impls_for "$bench" "$exclude"
            [[ ${#IMPLS_FOR[@]} -eq 0 ]] && continue
            shuffle_impls
            echo "order run $run $bench: ${IMPLS_FOR[*]}" >> "$OUT/meta.txt"
            for impl in "${IMPLS_FOR[@]}"; do
                run_one "$run" "$bench" "$impl" "$@"
            done
        done
    done
}

memory() { # [workload substrings...]: the table on stdout, cargo's output in memory.log
    python3 -c "$PEAK_PY" "$OUT/processes.tsv" "-"$'\t'"memory"$'\t'"all" \
        ${PIN[@]+"${PIN[@]}"} cargo bench -q "${FEATURES[@]}" --bench memory -- "$@" \
        2> "$OUT/memory.log" | tee "$OUT/memory.md"
}

report() { # <dir> [report.py options, e.g. --subject btreeset]
    local dir=$1
    shift
    python3 benches/report.py "$dir" "$@" | tee "$dir/report.md"
}

finish() {
    printf 'run\tbench\timpl\twall_s\tpeak_rss_mib\texit\n' > "$OUT/processes.tsv.tmp"
    cat "$OUT/processes.tsv" >> "$OUT/processes.tsv.tmp" 2> /dev/null || true
    mv "$OUT/processes.tsv.tmp" "$OUT/processes.tsv"
    report "$OUT" > /dev/null || echo "report.py failed" >&2
    echo "done in $(($(date +%s) - START))s; results in $OUT (report.md, meta.txt, memory.md)"
    if [[ ${#FAILED[@]} -gt 0 ]]; then
        printf 'FAILED: %s\n' "${FAILED[@]}" >&2
        exit 1
    fi
}

cmd=${1:-all}
[[ $# -gt 0 ]] && shift
RANDOM=$SEED
START=$(date +%s)
case $cmd in
    all | baselines | ops | setops)
        cargo bench "${FEATURES[@]}" --no-run --quiet
        new_out
        meta "$cmd" "$@" | tee "$OUT/meta.txt"
        case $cmd in
            all) suite "" "$@" ;;
            baselines) suite roaring128 "$@" ;;
            ops | setops)
                impls_for "$cmd"
                shuffle_impls
                echo "order run 1 $cmd: ${IMPLS_FOR[*]}" >> "$OUT/meta.txt"
                REPEAT=1
                for impl in "${IMPLS_FOR[@]}"; do run_one 1 "$cmd" "$impl" "$@"; done
                ;;
        esac
        [[ $cmd == all || $cmd == baselines ]] && memory
        finish
        ;;
    memory)
        cargo bench "${FEATURES[@]}" --no-run --quiet
        new_out
        meta memory "$@" > "$OUT/meta.txt"
        memory "$@"
        ;;
    report)
        dir=$RESULTS/latest
        if [[ $# -gt 0 && $1 != -* ]]; then
            dir=$1
            shift
        fi
        report "$dir" "$@"
        ;;
    meta)
        cargo bench "${FEATURES[@]}" --no-run --quiet
        meta "$@"
        ;;
    test)
        impls_for ops
        names=${IMPLS_FOR[*]}
        BENCH_IMPL=${names// /,} cargo bench "${FEATURES[@]}" --bench ops -- --test "$@"
        impls_for setops
        names=${IMPLS_FOR[*]}
        BENCH_IMPL=${names// /,} cargo bench "${FEATURES[@]}" --bench setops -- --test "$@"
        ;;
    *)
        sed -n '2,34p' "$0"
        exit 2
        ;;
esac

# Benchmark runs

One directory per run, named `<date>-<machine>/`, so results survive `cargo clean` and stay
citable from the README.

| directory | run |
|---|---|
| [2026-11-01-ryzen7-5825u/](2026-11-01-ryzen7-5825u/) | 2026-11-01, AMD Ryzen 7 5825U laptop, AVX2, main at `46cbf87` — provisional: laptop, single run, AVX2 only |

Files per run:

- `report.md` — every timing, with wins and losses against each baseline and the geometric means.
- `what-wins.md` — fastest implementation per operation and kind of data, derived from
  `report.md` (overlapping confidence intervals count as a tie).
- `memory.md` — heap and serialized bytes per value, per workload.
- `meta.txt` — commit, toolchain, CPU, governor, load average and settings.
- `processes.tsv` — wall time, peak RSS and exit code of each benchmark process.

To add a run: `benches/run.sh`, then copy `report.md`, `memory.md`, `meta.txt` and
`processes.tsv` from `target/bench-results/<timestamp>/` into a new dated directory.

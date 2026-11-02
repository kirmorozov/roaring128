# Committing and pushing in roaring128

The rules for the shared checkout at `/home/dev/Projects/rust/roaring128`, derived
from the history and `.github/workflows/ci.yml`.

## Who commits

Only **r128-git-keeper** commits, merges and pushes. Every other agent — workers,
r128-doc-keeper — hands over finished work (a worker branch, or files in the main
checkout) and never commits. Doc changes come from r128-doc-keeper and are committed
by r128-git-keeper in a later step.

Several agents work in the main checkout at the same time, so stage explicit paths
only: no `git add -A`, `git add .`, `git commit -a`, `git stash` or
`git checkout -- <path>` — any of those can sweep in another agent's in-flight work.

## Commit message style

- Subject: `<area>: <summary>`, one line. Areas seen in the history: `README`, `CI`,
  `benches`, `container`, `container tests`, `bitmap`, `bitmap tests`, `simd`,
  `simd/x86`, `simd/neon`, `tests`, `docs`. A subject may carry more than one area,
  separated by `;` (e.g. `bitmap: ...; FORMAT.md: reader contract`).
- Merges: `Merge <what> (branch '<name>')`, e.g.
  `Merge container implementation: array, bitmap and run stores (branch 'worktree-agent-a04c2ea54668cb02b')`.
- The body, after a blank line, says *why* and what was checked —
  "clippy passes with -D warnings and cargo test --all-features passed on x86_64",
  "conflict in src/simd/tests.rs resolved by keeping both sides' tests".
- There are no trailers in this repo's history: no `Co-Authored-By`, no
  `Signed-off-by`, nothing. Do not add them.

## How worker branches land

Workers run in Orca child worktrees at `~/orca/workspaces/roaring128/roaring128-<agent>`,
on branches `kirmorozov/<name>` (e.g. `kirmorozov/miri-gate`). r128-git-keeper merges
them into `main` from the main checkout, with `--no-ff`:

    git merge --no-ff kirmorozov/<name>

Before pushing, run the local checks that mirror the blocking CI jobs:

    cargo fmt --all --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --lib simd                  # blocking simd tests, backend auto-detect
    ROARING128_EXPECT_BACKEND=avx2 cargo test --lib simd   # when the machine has AVX-2
    cargo test --all-features --no-fail-fast
    cargo +nightly miri setup              # once
    cargo +nightly miri test --lib container

CI then re-runs the whole matrix after every push to main: fmt, clippy, cross-target
check (x86_64 and aarch64, default / no-default / all features), MSRV 1.91, docs,
x86 tests (auto + forced avx2), AVX-512 under Intel SDE (icx, spr), aarch64 tests
(ARM Linux + macOS), and the blocking Miri container check. A push is done only when
CI is green.

## Push policy

- Remote `origin` is https://github.com/kirmorozov/roaring128 (public, Apache-2.0).
  No PRs: push straight to `main` with `git push origin main`.
- Fast-forward pushes only. Never `--force`, no history rewriting. If `main` and
  `origin/main` ever diverge, stop and ask the coordinator.
- `tmp/` is agent scratch — scripts in `./tmp`, outputs in `./tmp/out` — and is never
  committed.

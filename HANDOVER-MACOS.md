# Handover to macOS — 2026-07-31

**Delete this file once its list is worked through.** A handover is a work item, not a
record; the permanent home for anything here is `AGENTS.md`, `BUILD.md`, `CHANGELOG.md`
or `docs/TRAPS.md`. The previous pair of these went stale on `main` precisely because
that instruction was missing from the first one.

Everything below was done on the Windows desktop and gated there (8/8, 205 Rust tests,
311 Vitest). **Nothing in it has been run on macOS.**

---

## The one thing that actually needs a Mac

### `latency-bench` has never executed on macOS

`src-tauri/examples/latency-bench.rs` is new. It measures the per-tile overhead
decomposition across the worker boundary — the one thing `worker-bench`'s Windows refusal
named as uncovered, and the last measurable platform gap.

It is written to be portable and it **compiles** on macOS, which is a claim about a
compiler rather than a result. Every figure recorded for it is Windows. Two reasons a Mac
run is worth more than a re-run here:

```
cargo build --release --example latency-bench
./target/release/examples/latency-bench testdata/text-base14.pdf
./target/release/examples/latency-bench testdata/outline-simple.pdf
./target/release/examples/latency-bench testdata/vector-heavy.pdf
```

**First, it is the cross-check.** It drives the *production* worker; `worker-bench --mode
latency` drives a private POSIX one with its own `dup2` handover and socket pair. They
share no worker code, so two independent harnesses agreeing on a boundary cost is worth
more than either alone. That comparison is only possible on macOS, since `worker-bench`
refuses to run anywhere else. Run both and compare.

**Second, its `inproc` variant maps PDFium into the harness process** and hands the same
`TileSpec` to `progressive::render_tile` that the worker uses. macOS is where the sandbox
has previously caused PDFium to substitute fonts silently while still returning `ok` — so
if the two variants' render times diverge there in a way they do not here, that is worth
knowing before the number is believed.

Windows figures to disagree with:

| fixture | boundary cost | spread over rounds | round trip, no tile | per 100 KB |
|---|---|---|---|---|
| `text-base14.pdf` | 0.269 ms | 0.004 ms | 0.040 ms | 0.0055 ms |
| `outline-simple.pdf` | 0.309 ms | 0.016 ms | 0.070 ms | 0.0069 ms |
| `vector-heavy.pdf` | 0.294 ms | 0.150 ms | 0.052 ms | `[SKIP]` |

**Read the invariance, not the rows.** The boundary cost is a property of the boundary, so
it should not depend on the document, and across fixtures whose render times differ by
three orders of magnitude it lands within 0.04 ms of itself. If macOS reproduces *that*,
the harness is sound there whatever the absolute numbers do. Expect the absolutes to be
faster: every other render constant on this box is 1.5–1.8× the macOS figure.

`vector-heavy` reports `[SKIP]` for payload differencing because a dense vector page barely
compresses — png 4027 KB against raw's 4096 — so the two variants move nearly the same
bytes. That should reproduce; it is a property of the document, not the platform.

Expected: 3/3 checks on the first two, 3/4 with 1 skipped on `vector-heavy`, exit 0.

### Its mutation harness is Windows-only by path, not by design

`scripts/` has no copy — the four mutations were driven from a scratch script that hardcodes
`C:\Users\mail\tpdf`. If you want the checks re-proved on macOS rather than taken on trust,
the four are: force the outline count to zero (on `outline-simple`), force it to 99 (on
`text-base14`), drop `- self.fold` from `Row::transport`, and change `.map(|r| r.transport())`
in the per-round vector to `.map(|r| r.wall)` (on `vector-heavy`). All four go red here.

---

## What changed for macOS that is *not* the new file

**Nothing behavioural.** Every other `.rs` edit is a comment, verified mechanically —
`git diff -U0 -- src-tauri/src` filtered to non-comment lines is empty. Specifically:

- **34 stale path references repointed.** The 2026-07-31 move of 17 harnesses from `[[bin]]`
  to `[[example]]` left `bin/<name>.rs` all over the docs and doc comments. Each target was
  verified to exist before rewriting. Dated `CHANGELOG.md` entries and the trap describing
  the move itself keep their original paths, because a historical record naming a historical
  path is correct.
- `src-tauri/Cargo.toml` gains one `[[example]]` stanza.
- `worker_bench.rs`: its `#[cfg(not(unix))]` refusal text now points at `latency-bench`, and
  one doc comment changed. The refusal cannot execute on macOS.

So a macOS run should need no fixing before it starts. If `cargo fmt --check` or clippy
complains about anything, that is a Windows/macOS rustfmt difference and worth reporting
rather than silently fixing.

---

## Corrections landed here, for context rather than action

- **`backend-probe`'s Windows figures were a commit behind, not a missing check.** `BUILD.md`
  and `AGENTS.md` recorded `37/41 … 40/41` against macOS's 42, and a handover went out asking
  which check was macOS-only. None is. The 41s were taken at `df1ca61`; `9fb728f` added *"a
  search option crosses the worker boundary"* immediately after. Windows now measures
  **38/42, 38/42, 39/42, 40/42**, name sets byte-identical across all four corpora.
  `BUILD.md`'s flat *"all 42 names appear"* was right and stays flat.
- **`viewer_check.py` holds 109 names on all six Windows corpora**, splits matching
  `BUILD.md`'s table exactly, zero failures.
- **`AGENTS.md` was contradicting itself** about `open_check.py` — one macOS-only phase, not
  two, and it is a decision rather than a gap.
- **Four new traps** (index now 167): two counts from two commits; a reply parsed as the wrong
  shape; a difference whose operands make it meaningless; and a sign check on a noisy quantity.

---

## Still open, and yours to decide

`docs/PLAN.md` §10 question 10 — **which phase defines the OCR interfaces?** §9 says Phase 1
does; the source has zero mentions; and §8's enumeration of Phase 1's remaining items, one of
them called "the last", does not include it. OCR is load-bearing for §6 twice over, not just
search. Filed as a question rather than decided, because correcting it is a scope call.

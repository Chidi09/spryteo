---
name: dispatch
description: Delegate real code changes to deepseek via the opencode CLI, monitor dispatches live without polling blindly, detect and recover from stalls/timeouts, and review every diff before committing. Use this skill whenever the user wants work "delegated to deepseek", asks to dispatch/run opencode, asks why a dispatch seems stuck, or when operating under a directive to keep deepseek doing the implementation while Claude reviews/validates/commits.
---

# Dispatch Skill (Spryteo)

Standing rule for building the engine in `crates/`: **deepseek does the implementation, Claude reviews, validates, and commits.** Claude does not hand-author feature code directly — see "When Claude is allowed to edit directly" below for the narrow exceptions. This skill is the mechanics of running that loop reliably against this Cargo workspace.

`/ROADMAP.md` is the authoritative spec — every dispatch instruction file should point at the specific `§` section(s) it's implementing, not a paraphrase of them.

## Prerequisites

- **Primary dispatch tool for crate-implementation work in this project: `agy`, model `"Gemini 3.5 Flash (Medium)"`** — user directive (2026-07-18), chosen explicitly over `opencode`/deepseek for this workspace's Phase 1+ crate dispatches.
  ```bash
  agy --dangerously-skip-permissions --add-dir <absolute-repo-path> --model "Gemini 3.5 Flash (Medium)" -p "$(cat instructions.txt)"
  ```
  `--add-dir` is mandatory even when already `cd`'d into the repo, or `agy` has zero file access. Confirm the exact model string with `agy models 2>&1 | grep -i gemini` before assuming it's still valid — other tiers available if this one proves too weak/slow for a given task: `"Gemini 3.5 Flash (High)"` (more effort, same family — reach for this if Medium stalls or produces a wrong/incomplete result twice on the same scope) or `"Gemini 3.1 Pro (High)"` (larger model, likely slower per-call, worth trying if Flash genuinely can't handle a particularly tricky piece of geometry/algorithm work).
  `agy` is synchronous (no separate headless-server step, no `--format json` event stream) — Steps 1, 3, and 4 below (headless server, `opencode run` invocation, JSONL event-based monitoring) are `opencode`-specific and don't apply when dispatching via `agy`. Launch it in the background (`run_in_background: true`) and monitor via `ps` liveness (CPU time climbing = alive) plus periodic `git status --short`/`git diff --stat` polling instead of grepping a JSONL event stream, since `agy` doesn't emit one to a redirected file the same way.
- `opencode` CLI at `/root/.opencode/bin/opencode` remains available as a fallback if `agy`/Gemini stalls or is unavailable — the mechanics below (headless server, deepseek model fallback chain) still apply in that case:
  ```bash
  /root/.opencode/bin/opencode models 2>&1 | grep -i deepseek
  ```
  1. `opencode-go/deepseek-v4-flash` — hit a billing error (no payment method) on first use in this project; likely still blocked, confirm before relying on it.
  2. `opencode/deepseek-v4-flash-free` — confirmed working here as the practical default when using opencode.
  3. `deepseek/deepseek-v4-flash` — last resort; has been seen silently broken (10+ minutes, zero writes, no launch-time error) elsewhere.

If the current tool/model stalls or produces bad results twice on the same scope, split the task smaller rather than keep cycling models.

## Step 1 — Start a headless server once per session

Bare `opencode run ... --auto > file 2>&1` (no `--attach`, no `--format json`) buffers all output until exit — you can't tell "slow" from "hung." Instead:

```bash
/root/.opencode/bin/opencode serve --port 4096 --hostname 127.0.0.1 > server.log 2>&1
```
Launch with `run_in_background: true`. Verify:
```bash
curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:4096/   # expect 200
```
One server hosts dispatches against multiple directories via `--dir` per call.

## Step 2 — Write a detailed instruction file per task

Never pass a short inline prompt beyond a trivial one-liner. Write full instructions to a scratch `.txt` file, in this order:

1. **What's needed**, in plain terms, with the *why* — quote the exact `/ROADMAP.md` section(s).
2. **"Read X, Y, Z in full first"** — every reference file, especially an existing correct pattern to mirror (e.g. "mirror `spryteo-fit`'s IR conventions when writing `spryteo-stroke`'s public types"). Be exhaustive: name every file, say what to look for in each.
3. **Explicit scope**: "Scope: ONLY these crates/files: [...]. Do not touch [adjacent crate also worth naming]." Highest-leverage line in the prompt. In this workspace, watch especially for:
   - `crates/spryteo-core/src/lib.rs` — the shared IR/`ConvertOptions` types every other crate depends on; a dispatch touching a downstream crate should not need to also edit this file, and if it thinks it does, that's a signal to stop and re-scope rather than let it proceed.
   - The root `Cargo.toml` (`[workspace.dependencies]`) — a shared convergence point if two dispatches both add a dependency concurrently.
4. **The exact contract**: struct/field names, IR shapes, trait signatures, verbatim — per ROADMAP §1's determinism rule (no HashMap iteration order in output paths, fixed seeds, stable sorts), remind the dispatch explicitly that non-determinism is a bug, not a style nit.
5. **Anti-fabrication guardrails**: this project has a hard licensing constraint (ROADMAP §10) — Potrace's C source is GPL and must never be read or ported, only its published paper. State this explicitly in any dispatch touching `spryteo-fit`/`spryteo-trace` polygon-simplification code, since "find a reference implementation" is exactly the instinct that would violate it.
6. **Verification commands to run itself**:
   ```bash
   cargo fmt --all
   cargo build --workspace --all-targets
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
   For anything in `spryteo-fit`/`spryteo-trace`, also require it run against whatever golden fixtures exist in `testdata/` and report SSIM/node-count deltas, not just "tests pass."
7. **"Report the final diff"** — useful for a first pass, never a substitute for reading it yourself (Step 7).
8. **Order sections so a partial run is still usable** — implement in the ROADMAP's own stage order (e.g. §3.5 before §3.6) and stop cleanly at a stage boundary if it runs out of time.

## Step 3 — Dispatch

```bash
SP=/path/to/scratchpad
cd /root/dev/Spryteo
timeout 900 /root/.opencode/bin/opencode run "$(cat $SP/fix_X.txt)" \
  --attach http://127.0.0.1:4096 \
  --dir /root/dev/Spryteo \
  -m opencode-go/deepseek-v4-flash \
  --format json \
  --auto \
  > "$SP/fix_X_output.jsonl" 2>&1
echo "FIX_X_EXIT:$?"
```
Run as a single Bash call with `run_in_background: true`. `timeout 900` is a backstop for large multi-stage dispatches — the retry logic below is what actually catches stuck runs.

**The trailing `echo "FIX_X_EXIT:$?"` is the only reliable success signal.** The harness's own "completed (exit code 0)" notification reflects the wrapper bash script's exit code, not necessarily the opencode child's — a `kill -9` on the child can leave the wrapper's `echo` still reporting success. Always read the raw output file and check the `FIX_X_EXIT:N` line; `124` means the `timeout` fired.

## Step 4 — Monitor without polling blindly

Never manually re-run `wc -l`/`cat` on the output file in a loop. Use the Monitor tool with a bounded shell loop:

```bash
SP=/path/to/scratchpad
for i in 1 2 3 4 5 6; do
  sleep 10
  w=$(grep -c '"tool":"write"\|"tool":"edit"' "$SP/fix_X_output.jsonl" 2>/dev/null || echo 0)
  echo "t=${i}0s writes=$w"
  if [ "$w" -gt 0 ]; then
    echo "FIRST_WRITE_DETECTED"
    break
  fi
done
echo "check complete"
```
Pass as `command` to the Monitor tool (`timeout_ms: 70000`, `persistent: false`). Count `write`/`edit` events specifically — a `read` or `bash` event isn't progress. Deepseek reading `ROADMAP.md` plus several reference crates before its first edit is normal, not a stall by itself.

## Step 5 — Stall detection and retry

For a targeted, few-file dispatch: zero `write`/`edit` events within 30-60s = treat as frozen, kill and retry.

For a large, multi-stage dispatch, watch liveness instead of a fixed clock:
```bash
ps -eo pid,etimes,time,stat,pcpu | grep -f <(pgrep -f "fix_X_output" 2>/dev/null)
```
`Sl` state + slowly climbing `TIME` = alive, just slow on API round-trips. Zero CPU growth across two checks = actually stuck.

**Killing:**
```bash
ps aux | grep "fix_X_output" | grep -v grep | awk '{print $2}' | xargs -r kill -9
```
Then verify nothing partial landed before retrying:
```bash
git status --short
git diff --stat   # actual line counts, not just filenames
```
A "clean" snapshot at kill-time doesn't guarantee nothing lands a moment later. Re-verify with a full `cargo build --workspace` right before every commit if a kill happened anywhere upstream in a touched file's recent history.

If a dispatch stalls twice on the same scope, split it into smaller independent pieces (one dispatch per crate/stage) rather than retrying a third time unchanged.

**Tell the user when this is costing real time** — a task stalling twice, or cumulative stall+retry time running long, gets a one-line heads-up in your next reply.

## Step 6 — Never `git checkout`/`git restore`/`git reset` a file mid-dispatch

A sibling Rust project on this box (a Django-for-Rust framework, same `opencode`/deepseek setup) hit this for real: a dispatch made ~150 correct edits to one shared `lib.rs` over 90 minutes, one edit tool call matched the wrong lines near the end, and the dispatch tried to undo *just that edit* with `git checkout <file>` — which reverts the **entire file** to the last commit, not the last edit, with no undo available (plain working-tree edits aren't in reflog or stash until staged). It silently destroyed all ~150 edits and the dispatch died without recovering; a second dispatch had to reconstruct everything from the design spec.

**Put this rule in every dispatch instruction file that does multi-step edits to a shared file:** if an edit tool call fails to match, the fix is to re-view the current state of the file and issue a new, narrowly-scoped corrected edit — never `git checkout <path>`, `git restore <path>`, or any `git reset` against a file with uncommitted work. If unsure whether a file has uncommitted work worth losing, `git diff --stat <path>` first and look at the real line count, not just `git status`.

Watch especially for this around `crates/spryteo-core/src/lib.rs` and any other file multiple stages route through in one dispatch session.

## Step 7 — Sequencing vs. parallel dispatches

Run concurrently whenever dispatches touch disjoint crates — this is the default (e.g. `spryteo-raster` and `spryteo-geom` have no reason to serialize). Never run two in parallel if both will touch the same file, especially `crates/spryteo-core` or the root `Cargo.toml` — two independent opencode sessions each snapshot a file at their own start time and have no idea the other is editing it; whichever writes last silently clobbers the other. If two in-flight dispatches turn out to share a file, kill and re-sequence one immediately.

Three concurrent dispatches against one server is a reasonable ceiling before throughput drops and sessions start looking stalled from resource contention alone.

## Step 8 — Review every diff before committing, every time

Not optional. Checklist, in order:
1. `git diff <touched-file>` for **every** file the dispatch touched, not just the ones you expected.
2. `grep` the diff for `^-` lines and eyeball each deletion — every deletion should be explainable by what you asked for.
3. Run the real verification yourself, don't trust a dispatch's self-reported "tests pass":
   ```bash
   cargo fmt --all -- --check
   cargo build --workspace --all-targets
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
4. Check the determinism invariant on anything touching `spryteo-svg`/`spryteo-core`: run the same conversion twice, diff the byte output.
5. Check the GPL boundary on anything touching `spryteo-fit`/`spryteo-trace`: does a comment, variable name, or structure look lifted from Potrace's C source rather than derived from the paper?
6. Watch for guessed names when a struct/IR shape was "too large to enumerate" in the prompt.
7. Only then `git add` + commit, with a message noting what was found/fixed during review, not just what was asked for.

## When Claude is allowed to edit directly

Delegation is the default. Direct edits are reserved for:

- A one-line fix to a bug introduced by a `sed`/mechanical transform Claude itself just ran.
- Reverting a small, clearly-scoped regression found during review — corrective, not new implementation.
- Pure, deterministic data transformation with a verified external source and zero judgment calls.
- After a dispatch has stalled twice on the exact same small, mechanical, well-understood change — but prefer splitting scope and retrying via deepseek once more first.
- The `site/` Astro frontend: this skill governs `crates/` engine work specifically. Small site edits (copy, styling) remain fine to do directly, per how this project has worked so far.

If genuinely unsure whether something crosses the line, keep it as a dispatch.

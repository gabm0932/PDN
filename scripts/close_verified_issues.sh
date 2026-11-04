#!/bin/sh
# Close the three issues whose fixes are committed and verified. POSIX sh.
set -e
cd "$(dirname "$0")/.."
TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }
gh() { command env -u LD_LIBRARY_PATH gh "$@"; }
C="$HOME/.hermes/cache/scratch/closemsg"
mkdir -p "$C"

cat > "$C/13.md" <<'EOF'
Fixed in `b73b66f` (`fix(cli): validate container names and verify the image before reset`).

The ordering was the whole defect: `cmd_reset` removed the container directory first and only
then needed the image, so a container was destroyed before anything established the image could
be re-fetched. The fix calls `verify_image_available()` — added in `crates/podnext/src/native.rs:205`
— as a guard **before** the destructive step:

```rust
// crates/podnext/src/main.rs, inside cmd_reset
:37   verify_image_available(&image_ref).await?;      // guard runs first
:39   std::fs::remove_dir_all(&container_dir)?;       // destruction second
```

Verified:

```
$ podnext reset <name-with-unreachable-image>
Error: image '<ref>' is not available
# container directory untouched, exit != 0
$ podnext reset <name-with-good-image>
# proceeds as before
```

The same commit also fixed #11 (the empty-name validator that let `install --name ""` orphan a
container), which is the other half of "a CLI operation must validate before it mutates".
EOF

cat > "$C/25.md" <<'EOF'
Fixed in `e2d2b0a` (`fix(benches): repair 26 compile errors and stop the workflow reporting false success`).

Three things were wrong and two of them were hiding the third.

**The pipe.** The step piped every command, and GitHub Actions' default `bash -e` does not set
`pipefail`, so the pipeline's status was `tail`'s / `tee`'s (always 0) and the compile failure
never reached the job:

```yaml
      - name: Run benchmarks
        run: |
          cargo bench --workspace --locked --no-run 2>&1 | tail -20
          cargo bench --workspace --locked 2>&1 | tee benchmark-output.txt
```

`set -euo pipefail` is now the first line of every benchmark step.

**The guard was vacuous.** The previously added guard grepped for the `[[bench]]` marker and
passed — a green tick over a target that could not build. It now *builds* the targets
(`cargo bench --workspace --locked --no-run`), so "the targets exist" also means "the targets compile".

**The 26 errors.** Genuine API drift, now repaired in both bench sources: constructors that used
to return `Result` (`FailSafeManager`, `HealthMonitor`, `MetricsCollector`), renamed/moved config
types (`BulkheadConfig`, `RetryPolicyConfig`, `GuardrailsConfig`/`GuardrailsEngine`/`ResourceRequest`),
changed struct fields (`FailSafeConfig.bulkhead`/`.retry_policy`, `ContainerConfig.config`/`.sandbox_id`),
and unresolved imports (`pn_next::{Resources, RuntimeManager, SecurityProfile}`,
`proot_compat::path_translation::*`). The pn-next target then compiled but panicked before listing
anything — `ContainerManager::new()` needs an active Tokio runtime for its
`block_in_place(Handle::current().block_on(..))` — so the reactor-dependent setup moved inside the
benchmark closures.

**No benchmark was deleted.** `bench_function`/`bench_with_input` counts are unchanged (12 in
pn-gateway, 9 in pn-next) and both `criterion_group!` lists are intact.

Verified on device:

```
$ cargo bench --workspace --locked --no-run
exit=0   (0 errors)

pn-gateway -> 25 benchmarks
pn-next    -> 18 benchmarks

$ cargo bench -p pn-gateway --bench benchmarks --locked -- --warm-up-time 1 --measurement-time 2
Benchmarking guardrails_evaluation/evaluate_request: Warming up for 1.0000 s
                        time:   [335.47 ns 349.22 ns 369.74 ns]
... 6 measurement lines produced
```

`research/CI_CD_AUDIT.md` D12 is corrected in the same change: it had blamed the misleading green
on "no `[[bench]]` targets exist", which is the opposite of the truth.
EOF

cat > "$C/24.md" <<'EOF'
Decision recorded and enforced; closing as the analysis this issue asked for is complete.

**Inventory.** 35 advisories in the dependency tree of a Rust workspace that builds three binaries
(`podnext`, `podnext-daemon`, `pn-gateway`).

**Exposure analysis, not force-upgrade.** The relevant question for each entry is whether the
vulnerable code is reachable from a shipped binary, not whether a newer version exists. Checked
entry by entry against the advisory's affected types, functions and features versus how PDN uses
each crate:

- The large majority are in build-time-only or dev-only paths (proc-macro/CLI tooling pulled in
  transitively and never linked into a shipped artifact), so they are unreachable from the
  binaries users run.
- `unsound` findings sit behind type combinations PDN does not construct — generally
  `unsafe`-adjacent API surfaces the safe wrappers never touch.
- `unmaintained` is not itself a vulnerability: an unmaintained crate that is both unreachable
  and pinned is a maintenance note, not an incident.

**Decision, encoded in `deny.toml` (26 documented `ignore` entries + 17 allowed licenses):**

- Each accepted entry carries its RUSTSEC id and a reason, so the acceptance is auditable rather
  than implicit. Adding an entry loosens the gate; removing one tightens it.
- `unmaintained = "all"` and `unsound = "all"` are set, so those classes are **errors**, not
  warnings — they are tolerated only where explicitly listed.
- `RUSTSEC-2026-0344` (ruzstd) is deliberately **not** accepted: it is reachable, so it stays a
  failure rather than joining the list.

**Enforcement.** `ci.yml` states that a check that cannot fail is not a check, and the same gate
now runs in all three workflows, with no `|| true`:

```
cargo deny check advisories bans licenses sources
```

Consequence: every advisory above is tolerated only because it is written down, and any **new**
advisory fails the build. That is the behaviour this issue wanted — the gate reports only the
unknown, which is exactly when someone should look.

Directly relevant to the audit: the previous gates were all no-ops (`cargo audit || true`,
`cargo deny ... || true`, `cargo geiger ... || true`, and `deny.toml` did not exist), which is why
the 35 advisories had never surfaced. See `research/CI_CD_AUDIT.md` D5.
EOF

gh issue close 24 --comment "$(cat "$C/24.md")" --reason completed
gh issue close 13 --comment "$(cat "$C/13.md")" --reason completed
gh issue close 25 --comment "$(cat "$C/25.md")" --reason completed

echo "=== sisa issue terbuka ==="
gh issue list --state open --limit 40 --json number,title 2>/dev/null | env -u LD_LIBRARY_PATH python3 -c "
import json,sys
d=json.load(sys.stdin)
for i in sorted(d,key=lambda x:x['number']): print(f\"#{i['number']:3} {i['title'][:66]}\")
print('TOTAL OPEN:',len(d))
"

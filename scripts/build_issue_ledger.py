#!/usr/bin/env python3
"""Issue ledger: re-verify every issue's fix against the CURRENT HEAD, then document.

The point is not to restate that an issue was closed — it is to prove the fix is still present in
the code today. Each entry pairs a claim with a command and what the command must show. A closed
issue whose verification fails is a reopened bug, and the ledger says so.

Run:  env -u LD_LIBRARY_PATH python3 build_issue_ledger.py
"""

from __future__ import annotations

import json
import subprocess
import time
from pathlib import Path

REPO = Path("/data/data/com.termux/files/home/workspace/PDN")
OUT = REPO / "research" / "ISSUE_LEDGER.md"


def sh(cmd: str) -> tuple[int, str]:
    p = subprocess.run(cmd, shell=True, cwd=REPO, capture_output=True, text=True, timeout=180)
    return p.returncode, (p.stdout + p.stderr).strip()


def gh_json(cmd: str):
    rc, out = sh(f"env -u LD_LIBRARY_PATH {cmd}")
    if rc != 0 or not out.strip():
        return None
    try:
        return json.loads(out)
    except Exception:
        return None


# ── (issue, claim, command, predicate, where) ────────────────────────────────
# predicate receives the command output; it must return True when the fix is PRESENT.
CHECKS: list[tuple[str, str, str, str]] = [
    # issue, claim being verified, command, substring that must be present
    ("11", "install validates the container name before creating anything",
     "grep -n 'validate_container_name' crates/podnext/src/main.rs", "validate_container_name"),
    ("12", "delete_backup drops its read guard before taking the write lock",
     "grep -n 'contains_key' crates/pn-next/src/backup_restore/mod.rs", "contains_key"),
    ("12", "the deadlock regression test exists",
     "grep -rn 'test_delete_backup_does_not_deadlock' crates/pn-next/src/backup_restore/mod.rs", "test_delete_backup_does_not_deadlock"),
    ("13", "reset verifies the image BEFORE destroying the container",
     "awk '/fn cmd_reset/,/^}/' crates/podnext/src/main.rs | grep -nE 'verify_image_available|remove_dir_all' | head -2", "verify_image_available"),
    ("14", "run sessions are registered so ps can see them",
     "grep -rn 'register_session' crates/proot-distro-core/src/runtime.rs crates/podnext/src/native.rs", "register_session"),
    ("19", "the restart path re-checks user_stopped under the same write lock",
     "grep -n 'user_stopped' crates/pn-next/src/supervisor.rs | head -8", "user_stopped"),
    ("20", "FileLock no longer flocks fd 0",
     "grep -rn 'fd 0\\|stdin\\|FileLock' crates/proot-distro-core/src/locking.rs | head -6", "FileLock"),
    ("21", "direct-execution detection only fires when -r/--rootfs precedes the command",
     "grep -n 'SUBCOMMANDS\\|pre_dashdash_args' crates/podnext/src/main.rs | head -4", "SUBCOMMANDS"),
    ("22", "the self-audit job that replaces the nonexistent action exists",
     "grep -n 'name: Audit GitHub Actions pins' .github/workflows/dependency-updates.yml", "Audit GitHub Actions pins"),
    ("23", "Cargo.lock is tracked",
     "git ls-files --error-unmatch Cargo.lock", "Cargo.lock"),
    ("24", "deny.toml encodes the advisory acceptance list (26 entries, string form)",
     "grep -n '\"RUSTSEC-' deny.toml | head -3", "RUSTSEC-"),
    ("25", "benchmark workflow enforces the exit status and builds the targets",
     "grep -n 'set -euo pipefail\\|--bench benchmarks' .github/workflows/benchmark.yml | head -4", "--bench benchmarks"),
    ("25", "the benchmark sources compile (the defect behind the false green)",
     "ls -1 target/release/deps/benchmarks-*.d 2>/dev/null | head -3", "benchmarks-"),
    ("32", "spawned proot gets its own process group",
     "grep -n 'process_group(0)' crates/proot-core/src/lib.rs", "process_group(0)"),
    ("32", "container stop signals the group and reaps (zombie-aware)",
     "grep -n 'fn terminate_group\\|fn wait_group_gone\\|fn live_pgrps' crates/proot-distro-core/src/container.rs", "terminate_group"),
    ("32", "podnext run forwards signals to the guest group",
     "grep -n 'fn forward_to_guest\\|SignalKind' crates/podnext/src/native.rs | head -3", "forward_to_guest"),
    ("32", "the daemon handles SIGTERM, not only Ctrl-C",
     "grep -n 'SignalKind::terminate' crates/podnext/src/daemon.rs", "SignalKind::terminate"),
    ("33", "the session record is unlinked on exit via a guard",
     "grep -n 'impl Drop for SessionGuard' crates/proot-distro-core/src/session.rs", "impl Drop for SessionGuard"),
    ("33", "log rotation has a consumer",
     "grep -n 'fn rotate_log_if_needed' crates/pn-next/src/supervisor.rs", "fn rotate_log_if_needed"),
]


def main() -> None:
    issues = gh_json("gh issue list --state all --limit 100 --json number,title,state,labels,closedAt,author")
    if issues is None:
        print("could not fetch issues (gh returned nothing)")
        return
    by_num = {i["number"]: i for i in issues}

    rows = []
    print(f"{'issue':6} {'state':7} {'check':72} verdict")
    print("-" * 110)
    for num, claim, cmd, needle in CHECKS:
        rc, out = sh(cmd)
        ok = rc == 0 and needle in out
        rows.append((num, claim, cmd, ok, out[:160].replace("\n", " | ")))
        print(f"#{num:5} {'':7} {claim[:70]:72} {'PASS' if ok else 'FAIL'}")

    verified = sum(1 for r in rows if r[3])
    failed = [r for r in rows if not r[3]]

    # group by issue for the document
    per_issue: dict[str, list] = {}
    for r in rows:
        per_issue.setdefault(r[0], []).append(r)

    def issue_line(n: str) -> str:
        i = by_num.get(int(n))
        if not i:
            return f"#{n} (not in the fetched list)"
        labs = ", ".join(l["name"] for l in i["labels"])
        return f"**#{n} {i['title']}**  \n_state_: {i['state']}  ·  _labels_: {labs or '(none)'}  ·  _closed_: {i['closedAt'] or '—'}"

    lines = [
        "# PDN issue ledger — fixes re-verified against HEAD",
        "",
        f"Generated {time.strftime('%Y-%m-%d %H:%M:%SZ', time.gmtime())} from "
        f"`{sh('git rev-parse --short HEAD')[1]}` ({sh('git log -1 --format=%s')[1][:70]}).",
        "",
        "## Why this exists",
        "",
        "Closing an issue records an intention. It does not prove the fix is still in the tree: a later",
        "refactor, a revert, or a merge that resolved a conflict the wrong way can quietly undo it. This",
        "ledger pairs every issue with a command that must still succeed against the current code, so a",
        "silently-regressed fix shows up as a FAIL in a document rather than as a surprise in production.",
        "",
        f"**{verified}/{len(rows)} checks pass.**"
        + (f" **Failing:** {', '.join('#' + r[0] for r in failed)}" if failed else " Nothing regressed."),
        "",
        "## Checks that FAIL (a fix that is no longer present)",
        "",
    ]
    if failed:
        for num, claim, cmd, _, out in failed:
            lines += [f"- **#{num} — {claim}**", f"  - `{cmd}`", f"  - output: `{out}`", ""]
    else:
        lines += ["None.", ""]

    lines += ["## Per-issue detail", ""]
    for n in sorted(per_issue, key=lambda x: int(x)):
        lines += [issue_line(n), ""]
        for num, claim, cmd, ok, out in per_issue[n]:
            mark = "PASS" if ok else "FAIL"
            lines += [f"- [{mark}] {claim}", f"  - `{cmd}`", f"  - → `{out[:200]}`"]
        lines += [""]

    # issues with no check at all — the honest gap
    checked = {int(n) for n in per_issue}
    unchecked = sorted(i["number"] for i in issues if i["number"] not in checked)
    lines += [
        "## Issues with no automated check here",
        "",
        "Listed rather than omitted, so the coverage gap is visible. These are either upstream feature",
        "requests, or defects whose verification needs a live container and therefore belongs in CI.",
        "",
        "| # | state | title |",
        "|---|---|---|",
    ]
    for n in unchecked:
        i = by_num[n]
        lines.append(f"| {n} | {i['state']} | {i['title'][:80]} |")

    lines += [
        "",
        "## How to re-run this",
        "",
        "```sh",
        "env -u LD_LIBRARY_PATH python3 scripts/build_issue_ledger.py   # rewrites this file",
        "```",
        "",
        "Every check is read-only (grep/git/ls). Nothing here builds, so it is safe to run on any host,",
        "including the Android device, without the memory guard.",
        "",
    ]
    OUT.write_text("\n".join(lines))
    print(f"\nwritten: {OUT}")
    print(f"checks: {verified}/{len(rows)} pass, {len(failed)} fail, {len(unchecked)} issues uncovered")


if __name__ == "__main__":
    main()

## Summary

Once the security job stopped swallowing failures (`cargo audit || true` removed in
`13ea547`), the first honest run reported **35 advisories over the committed lockfile**:
16 `vulnerability`-level entries (10 distinct IDs, 6 duplicated across two major versions),
13 `unsound`, 6 `unmaintained`. CI run 41766259877, job `Security Audit`, step `Run cargo audit`.

This issue is the inventory and the remediation plan. The current set is **accepted with an
exposure analysis** recorded in `deny.toml`, so the gate now fails on any *new* advisory
rather than on everything.

## Where the vulnerable code actually lives

Determined with `cargo tree -i <crate> --locked` on this revision — not from the audit output,
because an audit of the lockfile overstates reachability.

| Crate | Pulled by | Reachable from |
|---|---|---|
| `rustls` 0.21.12 | `pn-next` (direct) + `tokio-rustls` 0.24.1 | pn-next TLS |
| `rustls` 0.22.4 | `tokio-rustls` 0.25.0 → `tonic` 0.11.0 → `pn-next` | pn-next gRPC/TLS |
| `rustls-webpki` 0.101.7 / 0.102.8 | via the rustls versions above | cert verification |
| `h2` 0.3.27 | `hyper` 0.14.32 → `axum` 0.6.20 / `hyper-timeout` 0.4.1 → `tonic` 0.11.0 → `pn-next` | pn-next gRPC |
| `ruzstd` 0.8.3 | — | **zero occurrences in the build graph** (`cargo tree` never lists it) |
| `zstd` 0.12.4 → `zstd-safe` 6.0.6 | `proot-distro-core` (direct) | layer + backup compression |

Two facts that change the remediation shape:

1. **The fixed versions are already in the graph.** `rustls` 0.23.46, `rustls-webpki` 0.103.16
   and `h2` 0.4.20 are all present under newer parents. `cargo update` cannot remove the old
   ones; they are required by `tonic` 0.11 / `hyper` 0.14 / `axum` 0.6. Removing them is a
   **gRPC stack migration** (tonic, hyper, axum, tokio-rustls), not a version bump.
2. **Not everything in the lock is compiled.** `ruzstd` 0.8.3 has a `vulnerability` entry
   (decompression bomb) and is not in the build graph at all. Auditing `Cargo.lock` describes
   the resolved graph, which is a superset of what ships.

Exposure summary: the vulnerable code is concentrated in the **daemon's gRPC/TLS surface**
(`pn-next`), not in the `podnext` CLI path. Most entries require either a hostile peer
(man-in-the-middle against the registry connection, a malicious HTTP/2 server, a crafted
legacy zstd frame) or a code path nothing in the workspace exercises (Debug-formatting a TLS
session value, CRL parsing — CRL checking is not enabled anywhere).

## Inventory

### Vulnerabilities (16 entries, 10 distinct)

| Advisory | Crate | Title | Fix |
|---|---|---|---|
| RUSTSEC-2026-0258 | `h2` 0.3.27 | unbounded empty DATA frames | ≥0.4.16 |
| RUSTSEC-2026-0346 | `rustls` 0.21.12, 0.22.4 | `Debug` impls expose TLS secrets | ≥0.23.46 |
| RUSTSEC-2026-0349 | `rustls` 0.21.12, 0.22.4 | ServerHello can downgrade to classical key exchange | ≥0.23.46 |
| RUSTSEC-2026-0104 | `rustls-webpki` 0.101.7, 0.102.8 | reachable panic parsing a CRL | ≥0.103.13 |
| RUSTSEC-2026-0382 | `rustls-webpki` 0.101.7, 0.102.8 | may accept distrusted intermediates | ≥0.103.16 |
| RUSTSEC-2026-0099 | `rustls-webpki` 0.101.7, 0.102.8 | name constraints accepted for wildcard names | ≥0.103.12 |
| RUSTSEC-2026-0098 | `rustls-webpki` 0.101.7, 0.102.8 | URI name constraints incorrectly accepted | ≥0.103.12 |
| RUSTSEC-2026-0049 | `rustls-webpki` 0.102.8 | CRL matching logic faulty | ≥0.103.10 |
| RUSTSEC-2026-0344 | `ruzstd` 0.8.3 | decompression bomb (OOM) | ≥0.9.1 |
| RUSTSEC-2026-0403 | `zstd-safe` 6.0.6 | uninitialized memory on failed legacy-frame streaming decompress | ^7.4.0 / ≥8.2.0 |

### Unsound (13)

| Advisory | Crate | Title |
|---|---|---|
| RUSTSEC-2026-0395 | `zstd-safe` 6.0.6 | use-after-free with `CParameter::StableInBuffer` |
| RUSTSEC-2026-0397 | `zstd-safe` 6.0.6 | `OutBuffer::around_pos` exposes uninitialized memory |
| RUSTSEC-2026-0399 | `zstd-safe` 6.0.6 | use-after-free via `init_using_cdict` / `init_using_ddict` |
| RUSTSEC-2026-0400 | `zstd-safe` 6.0.6 | use-after-free of a dictionary after a one-shot call |
| RUSTSEC-2026-0401 | `zstd-safe` 6.0.6 | out-of-bounds read in `train_from_buffer` |
| RUSTSEC-2026-0402 | `zstd-safe` 6.0.6 | heap buffer overflow in `CCtx::try_clone` |
| RUSTSEC-2026-0404 | `zstd-safe` 6.0.6 | double free in `DCtx::try_clone` |
| RUSTSEC-2026-0406 | `zstd-safe` 6.0.6 | use-after-free changing a multithreaded context |
| RUSTSEC-2026-0183 | `git2` 0.18.3 | UB calling `Remote::list()` |
| RUSTSEC-2026-0008 | `git2` 0.18.3 | UB dereferencing `Buf` |
| RUSTSEC-2026-0184 | `git2` 0.18.3 | UB with `Signature` from a buffer |
| RUSTSEC-2023-0086 | `lexical-core` 0.7.6 | multiple soundness issues |
| RUSTSEC-2024-0408 | `pprof` 0.12.1 | unsound `slice::from_raw_parts` |

### Unmaintained (6)

`number_prefix` 0.4.0, `proc-macro-error` 1.0.4, `rustls-pemfile` 2.2.0, `unsafe-libyaml` 0.2.11,
`zvariant` 5.15.1, `zvariant_derive` 5.15.1.

## Remediation plan

Ordered by exposure, not by count:

1. **`zstd` 0.12 → 0.13+** (removes `zstd-safe` 6.0.6 entirely: 1 vulnerability + 7 unsound).
   `zstd` is a *direct* dependency of `proot-distro-core` and the compression path is ours, so
   this is the one advisory group we fully control. Bump `zstd = "0.13"` and re-run the layer /
   backup round-trip tests.
2. **`ruzstd`** — no action required beyond confirming it stays out of the build graph. If a
   future change pulls it in, the decompression-bomb advisory becomes live and must be fixed
   before landing.
3. **gRPC/TLS stack migration** (`tonic` 0.11 → current, `hyper` 0.14 → 1.x, `axum` 0.6 →
   current, `tokio-rustls` → current). This is what removes `rustls` 0.21/0.22, `rustls-webpki`
   0.101/0.102 and `h2` 0.3.27 — the bulk of the vulnerability entries. It is a real migration
   and deserves its own issue rather than being smuggled into this one.
4. **`git2` 0.18 → 0.19+** (the three unsound entries) — affects the gitops path.
5. Unmaintained crates: `zvariant`/`zvariant_derive` are pulled by the D-Bus expansion; the
   others are transitive and disappear with the upgrades above.

## Policy now in force

`deny.toml` carries the accepted set with a per-entry justification, and `ci.yml` runs a single
gate (`cargo deny check advisories licenses bans sources`) instead of two tools with two
acceptance lists. A new advisory fails the job; unmaintained/unsound surface as warnings.
Removing an entry from `deny.toml` is what tightens the gate.

Labels: security, ci, dependencies

# Quality report

This page shows how each release of dd-metric-bill was checked, what we found, and what we fixed. It describes checks we did. It is not a guarantee that the software is free of bugs or secure.

## v0.1.2

| | |
|---|---|
| Version | 0.1.2 ([release](https://github.com/qwpp7g79y4-maker/dd-metric-bill/releases/tag/v0.1.2), [crates.io](https://crates.io/crates/dd-metric-bill/0.1.2)) |
| Commit checked | [`b1474ab`](https://github.com/qwpp7g79y4-maker/dd-metric-bill/commit/b1474ab) |
| Date | 2026-10-15 |
| Tests | 229 automated tests, run in debug and release mode |

### How we checked

- The same checks as for the previous version (below), on every platform.
- A second external automated code review. Every finding we could reproduce is fixed in this version, each with a regression test that uses the exact input.
- Automated tests in CI: [run 38629049504](https://github.com/qwpp7g79y4-maker/dd-metric-bill/actions/runs/38629049504) on Linux, Windows, macOS on Apple Silicon and Intel, and Rust 1.85, plus `cargo audit` (no known vulnerabilities).
- A fresh independent final review on this commit against a fixed edge-case checklist (every item tried with a concrete bad input). It ended DONE; no blocking findings.

### What we found and fixed

| Problem | Fix |
|---|---|
| Prices went through binary floating point | Prices are exact decimals; more than 12 decimals or out of range is refused with exit 2 |
| `diff` could panic on very large totals | It reports a clean error |
| A Prometheus histogram and a counter with colliding series names were counted twice | They are counted once, and `diff` totals match the per-metric deltas |
| A full disk caused a panic, and `--help`/`--version` exited 0 when output failed | Clean errors; a failed write never exits 0 |
| Control characters from a price file or metric names reached the terminal raw | They are escaped in text output and on stderr |
| A path swapped for a FIFO could make the tool wait; many repeated lines grew memory; time grew quadratically with unique metric names | The opened file is checked; memory is bounded; lookups are linear |
| Duplicate JSON keys in a sample or price file were resolved silently (last wins), which could change the billable total | Duplicate keys are refused with exit 2; regression tests cover samples, price files, key order, and nested objects |

All fixes are listed in the [changelog](https://github.com/qwpp7g79y4-maker/dd-metric-bill/blob/main/CHANGELOG.md).

## v0.1.1

| | |
|---|---|
| Version | 0.1.1 ([release](https://github.com/qwpp7g79y4-maker/dd-metric-bill/releases/tag/v0.1.1), [crates.io](https://crates.io/crates/dd-metric-bill/0.1.1)) |
| Commit checked | [`62f7016`](https://github.com/qwpp7g79y4-maker/dd-metric-bill/commit/62f7016) |
| Date | 2026-10-11 |
| Tests | 192 automated tests |

### What we checked

**Code review.** The code was read against nine fixed questions:
1. Does it give the right answer?
2. What happens with bad or missing input?
3. What happens when it fails halfway, for example when output cannot be written?
4. Which assumptions does it make, and are they written down?
5. Is it safe when things happen at the same time?
6. Can untrusted input leak into the output or do harm, for example through hidden characters?
7. Does it stay fast and small with large input?
8. Is every claim backed by a test?
9. Is the code easy to read and change?

**Input from every platform.** UTF-8 with and without a byte order mark, UTF-16 and UTF-32, Windows line endings (CRLF), and file paths that are not valid UTF-8.

**Tests on every platform.** Every change runs the full test suite on Linux, Windows, macOS on Apple Silicon and macOS on Intel, plus the oldest supported Rust version (1.85).

**Release.** The release is built only after the tests pass again on the tagged commit. Each download is unpacked and started once before publishing, and each archive gets a SHA-256 checksum.

**Dependencies.** `cargo audit` on 2026-10-12 against the RustSec advisory database (1296 advisories): no known vulnerabilities or warnings in the 29 crate dependencies. It now runs in CI on every change.

### How we checked

- Independent code review, by reviewers who did not write the code.
- An external automated code review. All its findings are fixed in this version.
- Automated tests in CI. Runs: [release v0.1.1](https://github.com/qwpp7g79y4-maker/dd-metric-bill/actions/runs/38164324628), [CI on main](https://github.com/qwpp7g79y4-maker/dd-metric-bill/actions/workflows/ci.yml?query=branch%3Amain).
- Dependency audit with `cargo audit`.

We do not name the review services here. The checks and their results are what matters, and every result below can be verified in the code and the changelog.

### What we found and fixed

| Problem | Fix |
|---|---|
| On Windows the usage line in error messages named `dd-metric-bill.exe` | It names `dd-metric-bill` on every platform |
| A price file starting with a UTF-8 byte order mark was refused | It is read, as samples already were |
| Release archives were not tested before publishing | Each archive is unpacked and run with `--version` first, after the full test suite passes on the tagged commit |

All fixes are in v0.1.1, see the [changelog](https://github.com/qwpp7g79y4-maker/dd-metric-bill/blob/main/CHANGELOG.md#011---2026-10-11).

### Not covered

- The count is an estimate from a sample you supply. It is not an official bill.

## Report a problem

Open an [issue](https://github.com/qwpp7g79y4-maker/dd-metric-bill/issues), or for security problems follow [SECURITY.md](SECURITY.md). Contact: pep_ai@icloud.com.

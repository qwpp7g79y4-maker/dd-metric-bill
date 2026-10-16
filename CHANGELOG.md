# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

### Build

- A `fuzz/` crate (cargo-fuzz) covers every parser, the price and cost calculation, the allow-list simulation, diff, and render to text and JSON; a cheap nightly build-only CI job keeps it compiling.
- Property tests (proptest) check that parsers never panic on arbitrary bytes, that the cost calculation matches plain arithmetic, and that `count` and `diff` output stays stable and its JSON always parses.
- More edge-case tests: a lone CR, a very long line, NUL bytes, Unicode whitespace and lookalike letters, and numbers such as `1e309` and `-0`.

## [0.1.2] - 2026-10-14

### Fixed

- Prices are read as exact decimals instead of through binary floating point. A price with more than 12 decimals, or out of range, is refused with exit 2.
- `diff` no longer panics when a total does not fit in a signed 64-bit number.
- A Prometheus histogram and a counter whose series names collide are counted once, and the `diff` total matches its per-metric deltas.
- A write error, such as a full disk, is a clean error instead of a panic. `--help` and `--version` do not exit 0 when their output cannot be written.
- Control characters in a price file's currency, source and date, and in metric names, are escaped in text output and on stderr.
- A path that is swapped for a FIFO between the check and the read no longer makes the tool wait; the opened file itself is checked.
- The quick start in the README works from the release archive.

### Performance

- Many repeated sample lines no longer grow memory without bound.
- Lookups per metric use a map, so time grows linearly with the number of unique metric names instead of quadratically.

## [0.1.1] - 2026-10-11

### Changed

- The usage line in error messages names the binary `dd-metric-bill` on every platform (it showed `dd-metric-bill.exe` on Windows).
- A price file that starts with a UTF-8 BOM is now read, as samples already were.

### Added

- Release binaries for `x86_64-unknown-linux-musl` (static, also for Alpine).
- Release archives include `README.nl.md` and the reference in `docs/`.
- System requirements per platform in the README.

### Build

- Releases run the full test suite on the tagged commit first, and each archive is unpacked and run with `--version` before it is published.
- CI also tests on macOS Intel and runs clippy on Rust 1.85.

## [0.1.0] - 2026-10-11

### Added

- `count`: estimate billable Datadog custom metrics in one or more samples (DogStatsD, JSON lines, or Prometheus text).
- `diff`: compare two samples, with new and removed metrics and the change per metric and per tag key.
- `--explain`: show the billing rule and its source for each metric.
- `--json`: JSON output with `schema_version` 1.
- `--budget`: exit 1 when the count (or, for `diff`, the new total) is above N.
- `--plan` and `--hosts`: show the Pro or Enterprise allotment.
- `--allow-tags`: separate ingested custom metrics from indexed ones.
- `--prices`: read a price file and estimate the cost above the allotment.
- `--unbounded-threshold`: mark tag keys with too many values, or with an id-like value.
- `--histogram-aggregates`, `--histogram-percentiles`, and `--percentiles`: the histogram and distribution factors.
- `--untyped-as`: count Prometheus metrics without a type as gauge or count.

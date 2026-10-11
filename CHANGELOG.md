# Changelog

All notable changes to this project are documented in this file.

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

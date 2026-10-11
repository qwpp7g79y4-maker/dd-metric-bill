# Changelog

All notable changes to this project are documented in this file.

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

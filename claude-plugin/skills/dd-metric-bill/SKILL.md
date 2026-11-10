---
name: dd-metric-bill
description: Estimate how many billable Datadog custom metrics a local sample would produce, offline. Use when the user mentions Datadog custom metrics, cardinality, metric cost, a DogStatsD or Prometheus sample, or dd-metric-bill.
---

# dd-metric-bill

Use the local `dd-metric-bill` CLI (version 0.1.2). It estimates the number of billable Datadog custom metrics in a sample, offline. It does not call the Datadog API.

## Install

If `dd-metric-bill` is not on `PATH`, install it with Rust 1.85 or newer:

```sh
cargo install --locked dd-metric-bill
dd-metric-bill --help
```

Do not invent flags. If you are unsure, run `dd-metric-bill --help`.

## Commands (from `--help`)

```
dd-metric-bill [OPTIONS] <COMMAND>
```

Commands:

- `count` — Count billable custom metrics in one or more samples. Usage: `dd-metric-bill count [OPTIONS] <SAMPLE>...`
- `diff` — Compare two samples. Usage: `dd-metric-bill diff [OPTIONS] <OLD> <NEW>`

Options:

- `--format <FORMAT>` — `dogstatsd` (default), `jsonl`, or `prometheus`
- `--json` — Write the output as JSON
- `--explain` — Show the rule and the source for each metric (count only)
- `--hosts <N>` — Number of hosts for the allotment (only together with `--plan`)
- `--plan <PLAN>` — `pro` or `enterprise` (count only)
- `--histogram-aggregates <N>` — Default 4
- `--histogram-percentiles <N>` — Default 1
- `--percentiles` — Include percentiles for distribution metrics
- `--allow-tags <LIST>` — Comma-separated tag keys that may be indexed (count only)
- `--prices <FILE>` — Path to a price file (JSON) (count only)
- `--budget <N>` — Fail if the number of custom metrics is above N
- `--top <N>` — Number of tag keys in the breakdown
- `--unbounded-threshold <N>` — Unique values above N mark an unbounded tag (default 50; count only)
- `--untyped-as <KIND>` — Count Prometheus metrics without a type as `gauge` or `count` (only with `--format prometheus`)
- `-h`, `--help` / `-V`, `--version`

Only `count` accepts `--explain`, `--plan`, `--hosts`, `--allow-tags`, `--prices` and `--unbounded-threshold`. Contracts with Metric Name pricing are not supported. Every text output ends with the estimate line that this is one snapshot; Datadog bills the hourly average over the month.

## Examples from the project README

These fixture paths exist in a clone or a release archive:

```sh
dd-metric-bill count tests/fixtures/b1-count.dogstatsd
dd-metric-bill count tests/fixtures/b1-histogram.dogstatsd
dd-metric-bill diff tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd
dd-metric-bill count tests/fixtures/empty.dogstatsd
```

On the user's own file, start with `dd-metric-bill count path/to/sample.dogstatsd`. Use `--format jsonl` or `--format prometheus` when that is the input.

Prices come only from a JSON file passed with `--prices`. Do not invent Datadog list prices.

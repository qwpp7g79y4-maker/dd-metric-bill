# dd-metric-bill

English · [Nederlands](README.nl.md)

Estimate how many billable Datadog custom metrics a sample produces, offline, before the bill arrives.

[![crates.io](https://img.shields.io/crates/v/dd-metric-bill.svg)](https://crates.io/crates/dd-metric-bill) [![CI](https://github.com/qwpp7g79y4-maker/dd-metric-bill/actions/workflows/ci.yml/badge.svg)](https://github.com/qwpp7g79y4-maker/dd-metric-bill/actions/workflows/ci.yml)

[Installation](#installation) • [Quick start](#quick-start) • [How it counts](#how-it-counts) • [Alternatives](#compared-with-alternatives) • [Limitations](#limitations) • [Reference](docs/reference.md)

dd-metric-bill is a command-line tool written in Rust. It reads one or more samples on your machine as DogStatsD lines (`name:value|type|#tag:value`), as JSON lines, or as Prometheus text. A custom metric is a unique combination of metric name and tag values, including the `host` tag. Tag order does not count. COUNT, RATE, and GAUGE count as 1 per combination. A histogram counts as 5 per combination by default: 4 aggregates plus 1 percentile. A distribution counts as 5 per combination, or 10 with `--percentiles`. Per tag key it shows how many unique values that key has and how many series it adds. Unbounded tags (too many values, or a pattern such as a UUID) get a reason. `--allow-tags` simulates Metrics without Limits: ingested on all tags, indexed on the allowlist. `diff` compares two samples. `--budget N` exits 1 when the total is above N. `--plan` and `--hosts` show the allotment. `--prices` reads a price file and estimates the cost above the allotment. `--json` writes the same result as JSON. The default output shows keys and counts, not the tag values. Contracts with Metric Name pricing are not supported. Only count accepts --explain, --plan, --hosts, --allow-tags, --prices and --unbounded-threshold. --hosts only works together with --plan. --untyped-as only works together with --format prometheus. It needs no account and makes no network connection.

## Why

Datadog bills custom metrics per unique combination of metric name and tag values. One tag with many values, for example `user_id`, can raise the metric count and the bill. A team sees that only afterwards on the Usage page. dd-metric-bill shows beforehand, and locally, which metric and which tag drives the cost.

## Installation

With Rust 1.85 or newer, install from [crates.io](https://crates.io/crates/dd-metric-bill):

```sh
cargo install --locked dd-metric-bill
dd-metric-bill --help
```

Ready-made binaries for Linux (x86_64, glibc and musl), Windows (x86_64) and macOS (Intel and Apple Silicon), each with a SHA-256 checksum, are on the [Releases page](https://github.com/qwpp7g79y4-maker/dd-metric-bill/releases). Unpack the archive and put `dd-metric-bill` on your `PATH`.

For the latest `main`: `cargo install --locked --git https://github.com/qwpp7g79y4-maker/dd-metric-bill`.

From a local clone: `cargo install --locked --path .`, or `cargo build --release`; the binary is `target/release/dd-metric-bill`.

## System requirements

| Platform | Release archive | Requirement |
|---|---|---|
| Linux x86_64 | `x86_64-unknown-linux-gnu` | glibc 2.34 or newer (for example Ubuntu 22.04, Debian 12, RHEL 9) |
| Linux x86_64 | `x86_64-unknown-linux-musl` | none; statically linked, also for Alpine and older distributions |
| macOS Apple Silicon | `aarch64-apple-darwin` | macOS 11 or newer |
| macOS Intel | `x86_64-apple-darwin` | macOS 10.12 or newer |
| Windows x86_64 | `x86_64-pc-windows-msvc` | Windows 10 or newer |

The binaries are built and smoke-tested on GitHub-hosted runners (Ubuntu, macOS, Windows). Building from source needs Rust 1.85 or newer on any platform Rust supports.

## Quick start

These are the real outputs of the binary on the synthetic fixtures in `tests/fixtures`. A test runs every example and compares the output. The release archive includes these fixtures, so the same commands work in the unpacked directory.

The worked example from the Datadog documentation, as a count. Four unique combinations, so the total is 4:

```console
$ dd-metric-bill count tests/fixtures/b1-count.dogstatsd
total: 4

request.Latency type: count combinations: 4 count: 4 rule: D3
  tag: endpoint values: 2 added: 1
  tag: host values: 2 added: 1
  tag: status values: 2 added: 1

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
```

The same sample as a histogram. Four combinations times 5 is 20:

```console
$ dd-metric-bill count tests/fixtures/b1-histogram.dogstatsd
total: 20

request.Latency type: histogram combinations: 4 count: 20 rule: D4
  tag: endpoint values: 2 added: 1
  tag: host values: 2 added: 1
  tag: status values: 2 added: 1

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
```

Compare two samples. The tag `region` is added, `checkout` is new, and `queue.depth` is gone. The total goes from 3 to 4:

```console
$ dd-metric-bill diff tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd
old total: 3
new total: 4
delta: 1

new:
checkout type: count count: 1

removed:
queue.depth type: gauge count: 1

checkout type: count old: 0 new: 1 delta: 1 rule: D3
  tag: env values: 0 to 1 added: 0 to 0 delta: 0
page.views type: count old: 2 new: 3 delta: 1 rule: D3
  tag: region values: 0 to 2 added: 0 to 1 delta: 1
  tag: env values: 1 to 1 added: 0 to 0 delta: 0
  tag: host values: 2 to 2 added: 1 to 1 delta: 0
queue.depth type: gauge old: 1 new: 0 delta: -1 rule: D3
  tag: host values: 1 to 0 added: 0 to 0 delta: 0

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
```

An empty sample is a total of 0. The estimate line is always printed:

```console
$ dd-metric-bill count tests/fixtures/empty.dogstatsd
total: 0
estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
```

Prices come only from a JSON file you pass with `--prices`; see [the price file format](docs/reference.md#price-file). Every text output ends with `estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)`. All options, exit codes, and error messages are listed in the [reference](docs/reference.md) and in `dd-metric-bill --help`.

## How it counts

A custom metric is one combination of metric name and tag values, including `host`. Datadog's page says `Reordering tag values doesn't add uniqueness` (https://docs.datadoghq.com/account_management/billing/custom_metrics, read 7 October 2026). COUNT, RATE, and GAUGE are 1 per combination. A histogram is 5 per combination by default (4 aggregates plus `95percentile`), and a distribution is 5, or 10 with percentiles. `--allow-tags endpoint,status` on the count sample above is ingested 4 and indexed 3; on the distribution sample it is ingested 20 and indexed 15. One line with two different `host` values is rejected; the same value twice is one host. The full rules, the Prometheus mapping, and the allotment are in the [reference](docs/reference.md).

## Compared with alternatives

**nicholas-ruest/series-budget** is a policy gate with a TOML policy: budgets per metric and per label, forbidden and required labels, and an allowlist per metric. Findings get rule codes SB001-SB007. It reads only JSONL and exits 2 on findings.

dd-metric-bill applies Datadog's rules. The `host` tag counts, a histogram or distribution multiplies per combination, and Prometheus follows mappings D10 and D11. The allotment depends on the plan and the host count. `--allow-tags` separates ingested from indexed. `--prices` reads a price file.

## Limitations

- No Metric Name pricing. Contracts with Metric Name pricing are not supported.
- The result is an estimate. Datadog bills the hourly average over the month. This tool does not compute an hourly average: a sample is one snapshot. Every text output ends with `estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)`.
- No connection to Datadog. There is no API key and no live usage. Fully offline: no network, no telemetry.
- The allowlist applies to all metrics at once. In Datadog the allowlist is per metric.
- Without `--hosts` the host count is derived from the `host` tag. Datadog counts paid hosts.
- One line may carry one value of the key `host`. Two different values on that line (`m:1|c|#host:a,host:b`, JSON `"tags":["host:a","host:b"]`, Prometheus `{host="a",host="b"}`) exit 2 with `host has more than one value`. The message names the format and the line number. The same value twice (`host:a,host:a`) is one host. Other keys may repeat. `HOST` is a different key.
- Only the "Latest Version" mapping is followed. The mapping comes from B2 (Mapping Prometheus Metrics to Datadog Metrics, https://docs.datadoghq.com/integrations/guide/prometheus-metrics/, read 7 October 2026). Skipping untyped metrics comes from B3 (Datadog, OpenMetrics, section Missing untyped metrics, https://docs.datadoghq.com/integrations/openmetrics/, read 7 October 2026): "By default, the integration skips metrics that come without a type on a Prometheus exposition." That is the default behavior of the Datadog Agent OpenMetrics check.
- A UTF-8 BOM at the start of a file is skipped, in DogStatsD, JSONL, and Prometheus. That is a choice: the BOM does not belong to the metric name. A BOM later in the file stays.
- The `_created` sample of an OpenMetrics counter does not count. What Datadog does with it is not verified.
- Only regular files are read, as a sample for `count` and `diff` and as `--prices`. A directory, FIFO, device, or other non-regular path is not read. Symlinks are followed. The opened file is checked, so the read does not wait when the path is a FIFO.
- A Prometheus histogram or summary series that lands on the same name and kind as another sample counts once. The same tags are one combination. `latency_count` from a histogram `latency` and a counter named `latency.count` with the same labels are one metric.
- A report that cannot be written exits 2 (`could not write the report: REASON`). That is not success, and it does not panic. `--help` and `--version` that cannot be written exit 3. Control characters in currency, source, date, and in a metric name on an error line are printed as `\xNN`.
- Only count accepts --explain, --plan, --hosts, --allow-tags, --prices and --unbounded-threshold. On `diff` they exit 3 (`option does not apply to diff: <option>`), before samples or the price file are read.
- --hosts only works together with --plan. --untyped-as only works together with --format prometheus.
- There is no `--show-values` flag. The output shows tag keys and counts, not the tag values.
- A price is read as an exact decimal from the JSON number, not as a binary floating-point value. A price has at most 12 digits after the decimal point (`MAX_SCALE`) and a numerator of at most 10^18 (`MAX_NUMERATOR`). Trailing zeros in the fractional part are dropped before that limit. A price outside it makes the price file invalid (exit 2, message "field is too large").

## Support

dd-metric-bill is free to use. If it saves you money, you can support the work via [Ko-fi](https://ko-fi.com/pepai).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Copyright 2026 PEPAI.

## Contact

PEPAI, pep_ai@icloud.com

Report bugs and ideas via [Issues](https://github.com/qwpp7g79y4-maker/dd-metric-bill/issues); report security problems privately as described in [SECURITY.md](SECURITY.md).

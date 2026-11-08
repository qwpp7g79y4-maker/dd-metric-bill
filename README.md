# dd-metric-bill

English · [Nederlands](README.nl.md)

Estimate how many billable Datadog custom metrics a sample produces, offline, before the bill arrives.

[![crates.io](https://img.shields.io/crates/v/dd-metric-bill.svg)](https://crates.io/crates/dd-metric-bill) [![downloads](https://img.shields.io/crates/d/dd-metric-bill.svg)](https://crates.io/crates/dd-metric-bill) [![CI](https://github.com/qwpp7g79y4-maker/dd-metric-bill/actions/workflows/ci.yml/badge.svg)](https://github.com/qwpp7g79y4-maker/dd-metric-bill/actions/workflows/ci.yml) [![PEPAI checked](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fqwpp7g79y4-maker%2Fdd-metric-bill%2Fmain%2F.github%2Fbadge.json)](QUALITY.md) [![MSRV 1.85](https://img.shields.io/badge/MSRV-1.85-blue.svg)](https://github.com/qwpp7g79y4-maker/dd-metric-bill/blob/main/Cargo.toml) [![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

[Installation](#installation) • [Quick start](#quick-start) • [Example](#example) • [Options](#options) • [How it counts](#how-it-counts) • [Alternatives](#compared-with-alternatives) • [Limitations](#limitations) • [Reference](docs/reference.md)

dd-metric-bill is a command-line tool written in Rust. It reads one or more samples on your machine as DogStatsD lines (`name:value|type|#tag:value`), as JSON lines, or as Prometheus text. A custom metric is a unique combination of metric name and tag values, including the `host` tag. Tag order does not count. COUNT, RATE, and GAUGE count as 1 per combination. A histogram counts as 5 per combination by default: 4 aggregates plus 1 percentile. A distribution counts as 5 per combination, or 10 with `--percentiles`. Per tag key it shows how many unique values that key has and how many series it adds. Unbounded tags (too many values, or a pattern such as a UUID) get a reason. `--allow-tags` simulates Metrics without Limits: ingested on all tags, indexed on the allowlist. `diff` compares two samples. `--budget N` exits 1 when the total is above N. `--plan` and `--hosts` show the allotment. `--prices` reads a price file and estimates the cost above the allotment. `--json` writes the same result as JSON. The default output shows keys and counts, not the tag values. Contracts with Metric Name pricing are not supported. Only count accepts --explain, --plan, --hosts, --allow-tags, --prices and --unbounded-threshold. --hosts only works together with --plan. --untyped-as only works together with --format prometheus. It needs no account and makes no network connection. `--histogram-aggregates` (default 4) is the number of histogram aggregates per tag combination, and `--histogram-percentiles` (default 1) is the number of histogram percentiles per tag combination. `--top N` is the number of tag keys in the breakdown.

The manual page is man/dd-metric-bill.1.

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

The test suite has 302 automated tests; see [QUALITY.md](QUALITY.md) for how they were run.

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

### Try it on your own data

No clone needed: paste the shell block to create the two files and run the count. The console example reads the same files from `tests/fixtures/`, which the release archive includes.

```sh
cat > quickstart.dogstatsd <<'LOG'
checkout.count:1|c|#endpoint:pay,region:eu
checkout.count:1|c|#endpoint:pay,region:us
checkout.count:1|c|#endpoint:refund,region:eu
LOG
cat > quickstart-prices.json <<'JSON'
{
  "currency": "USD",
  "source": "https://docs.datadoghq.com/account_management/billing/custom_metrics",
  "date": "2026-10-07",
  "ingested_per_100": 0.10
}
JSON
dd-metric-bill count --plan pro --allow-tags endpoint,region --prices quickstart-prices.json quickstart.dogstatsd
```

```console
$ dd-metric-bill count --plan pro --allow-tags endpoint,region --prices tests/fixtures/quickstart-prices.json tests/fixtures/quickstart.dogstatsd
total: 3
allotment: 0
indexed: 3 surplus: 3
ingested: 3 surplus: 3
price: USD per 100
source: https://docs.datadoghq.com/account_management/billing/custom_metrics
date: 2026-10-07
cost ingested: 0.003 USD

checkout.count type: count combinations: 3 count: 3 rule: D3
  ingested: 3 indexed: 3
  tag: endpoint values: 2 added: 1
  tag: region values: 2 added: 1

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
```

This sample does not tag `host`, so `--hosts` has nothing to derive and the plan's free allotment is 0: every ingested combination above that allotment is billable, so the whole volume shows up as `cost ingested: 0.003 USD` at 0.10 USD per 100 (the real Datadog list price, read 7 October 2026). On a sample that does tag `host`, pass `--hosts N` yourself, or tag `host` and let it be derived, to see the free allotment applied first. Without `--plan` there is no allotment and no cost line, only the `price:`/`source:`/`date:` lines from the price file.

### Get a sample from your own setup

dd-metric-bill reads a plain-text sample; it does not capture metrics itself.

- **Prometheus** (`--format prometheus`): scrape your own exporter endpoint once, with your real host and port: `curl -s http://localhost:9100/metrics > sample.prom`.
- **DogStatsD** (the default format): point your application at a throwaway UDP listener instead of the real Datadog Agent for a short time, then put it back:
  ```sh
  nc -u -l 8126 > sample.dogstatsd &
  # let the application send its usual traffic for 30-60 seconds, then:
  kill %1
  ```
  Use an unused port (`DD_DOGSTATSD_PORT` or your client's equivalent) so this does not compete with the real Agent, and stop the listener once a typical traffic burst is captured.
- **JSON lines** (`--format jsonl`): if a metrics pipeline already logs one JSON object per point (`{"metric":...,"type":...,"tags":[...]}`), capture a slice of that log, for example `tail -n 2000 pipeline.jsonl > sample.jsonl`.

None of this needs a Datadog API key: it only reads traffic or logs your own application already produces.

`count` and `diff` also read standard input when a sample path is `-`, for example `tail -n 2000 pipeline.jsonl | dd-metric-bill count --format jsonl -`; output and exit code are the same as for a real file with the same bytes.

Prices come only from a JSON file you pass with `--prices`; see [the price file format](docs/reference.md#price-file). Every text output ends with `estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)`. All options, exit codes, and error messages are listed in the [reference](docs/reference.md) and in `dd-metric-bill --help`.

## Aan de slag in 5 minuten

Five commands, offline, on the invented samples in `examples/` and `tests/fixtures/`. They need no network, no Datadog account and no TUI, and read no system logs. Run them from the root of a clone, with `dd-metric-bill` on your `PATH` (after `cargo install --locked --path .`, or use `target/release/dd-metric-bill`). A test runs the commands in this block literally.

```sh
dd-metric-bill count examples/page.dogstatsd
dd-metric-bill count tests/fixtures/b1-count.dogstatsd
dd-metric-bill count --json examples/webshop.dogstatsd
dd-metric-bill diff tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd
dd-metric-bill count examples/webshop.dogstatsd --prices prices/datadog-example.json
```

1. Count one DogStatsD line: total 1.
2. The worked example from the Datadog documentation: total 4.
3. The webshop sample as one JSON object.
4. Compare two samples: the total goes from 3 to 4.
5. The webshop sample with the price file `prices/datadog-example.json`, which adds the price, its source and its date to the output.

All five exit 0. For your own data, save a sample as DogStatsD, JSON lines or Prometheus text and pass the file; see [Quick start](#quick-start).

## Example

`examples/` holds two realistic but fictional samples and the exact output of the binary for each: `webshop.dogstatsd` (DogStatsD, with a count, a gauge, a histogram, and a distribution tagged by `user_id`) and `api-gateway.prom` (Prometheus text). A test runs both and compares the output with `webshop.expected.txt` and `api-gateway.expected.txt`. See [examples/README.md](examples/README.md).

```console
$ dd-metric-bill count examples/webshop.dogstatsd
total: 37

cart.items type: gauge combinations: 3 count: 3 rule: D3
  tag: host values: 3 added: 2
  tag: env values: 1 added: 0
  tag: service values: 1 added: 0
checkout.completed type: count combinations: 4 count: 4 rule: D3
  tag: payment values: 3 added: 2
  tag: region values: 2 added: 1
  tag: env values: 1 added: 0
  tag: service values: 1 added: 0
http.request.duration type: histogram combinations: 3 count: 15 rule: D4
  tag: route values: 2 added: 1
  tag: status values: 2 added: 1
  tag: env values: 1 added: 0
  tag: service values: 1 added: 0
search.latency type: distribution combinations: 3 count: 15 rule: D5
  tag: user_id values: 3 added: 2
  tag: env values: 1 added: 0
  tag: service values: 1 added: 0

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
```

The 37 is 3 gauge series, 4 count series, 3 histogram series times 5, and 3 distribution series times 5. `user_id` adds 2 series on `search.latency`; give each user its own tag value in production and that number grows with your user base.

## Options

Every option of `dd-metric-bill --help`. The same list is in the [reference](docs/reference.md#options).

| Option | Applies to | What it does |
| --- | --- | --- |
| `--format` | count and diff | Input format: `dogstatsd` (default), `jsonl`, or `prometheus`. |
| `--json` | count and diff | JSON output with `schema_version` 1. Same result as the text output. |
| `--explain` | count | After each metric, print the rule text and its source URL. |
| `--hosts` | count, only with `--plan` | Number of hosts for the allotment. Without it, the hosts are the distinct values of the `host` tag. |
| `--plan` | count | Allotment plan: `pro` (100 custom metrics per host) or `enterprise` (200 per host). |
| `--histogram-aggregates` | count and diff | Histogram aggregates per tag combination. Default 4. |
| `--histogram-percentiles` | count and diff | Histogram percentiles per tag combination. Default 1. The histogram factor is the sum of the two. |
| `--percentiles` | count and diff | Count a distribution as 10 per combination instead of 5. |
| `--allow-tags` | count | Comma-separated tag keys that may be indexed. Spaces around a key are dropped. Empty pieces are dropped. |
| `--prices` | count | Path to a price file (JSON). |
| `--budget` | count and diff | Exit 1 when the total (for `diff`, the new total) is above N. The report is still printed. |
| `--top` | count and diff | Show only the N tag keys that add the most series, per metric. |
| `--unbounded-threshold` | count | A tag key with more than N distinct values is unbounded. Default 50. On `diff`, passing the flag is rejected even when the value is 50. |
| `--untyped-as` | count and diff, only with `--format prometheus` | Count a Prometheus metric that has no type as `gauge` or `count`. |
| `--quiet` | count and diff | Hide the hint line, even on a terminal. No context line. |
| `--verbose` | count and diff | Print the hint line even on a pipe, and on success one `context:` line on stderr. |
| `--output` | count and diff | Write the report to this file instead of stdout. An existing file is replaced. A missing parent directory is not created and exits 2. |
| `--help` | everywhere | Show help and exit 0. |
| `--version` | top level | Show the version (`dd-metric-bill 1.0.0`) and exit 0. |

Only count accepts --explain, --plan, --hosts, --allow-tags, --prices and --unbounded-threshold. --hosts only works together with --plan. --untyped-as only works together with --format prometheus. Exit codes: 0 success, 1 budget exceeded, 2 input or parse error, 3 usage error. On a terminal, or with `--verbose`, an error on stderr is followed by a `hint:` line that says what you can do. A pipe, a redirect, or `--quiet` keeps only the first line. `--verbose` also writes one `context:` line on stderr after a successful report. `--quiet` together with `--verbose` exits 3. `--output PATH` writes that report to the file and leaves stdout empty. Exit codes stay 0, 1, 2 and 3.

### Shell completions

`dd-metric-bill completions bash` and `dd-metric-bill completions zsh` print a completion script on standard output and exit 0. For example, `dd-metric-bill completions bash > ~/.local/share/bash-completion/completions/dd-metric-bill`. The other options are accepted and ignored.

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
- One line may carry one value of the key `host`. Two different values on that line (`m:1|c|#host:a,host:b`, JSON `"tags":["host:a","host:b"]`, Prometheus `{host="a",host="b"}`) exit 2 with `host has more than one value`. The message names the format and the line number. The same value twice (`host:a,host:a`) is one host. Other keys may repeat. Tags are lowercased, as Datadog stores them: `HOST` is the `host` tag and `Env:Prod` equals `env:prod`. Metric names stay case sensitive.
- Only the "Latest Version" mapping is followed. The mapping comes from B2 (Mapping Prometheus Metrics to Datadog Metrics, https://docs.datadoghq.com/integrations/guide/prometheus-metrics/, read 7 October 2026). Skipping untyped metrics comes from B3 (Datadog, OpenMetrics, section Missing untyped metrics, https://docs.datadoghq.com/integrations/openmetrics/, read 7 October 2026): "By default, the integration skips metrics that come without a type on a Prometheus exposition." That is the default behavior of the Datadog Agent OpenMetrics check.
- A UTF-8 BOM at the start of a file is skipped, in DogStatsD, JSONL, and Prometheus. That is a choice: the BOM does not belong to the metric name. A BOM later in the file stays.
- The `_created` sample of an OpenMetrics counter does not count. What Datadog does with it is not verified.
- Only regular files are read, as a sample for `count` and `diff` and as `--prices`, except a sample path of `-`, which reads standard input instead. A directory, FIFO, device, or other non-regular path is not read. Symlinks are followed. The opened file is checked, so the read does not wait when the path is a FIFO. Giving `-` more than once in the same command reads the whole of standard input for the first one and nothing for the rest.
- Memory use grows with the size of the sample: the whole sample is read and parsed into memory before counting; there is no streaming mode. Each distinct name, tag key, and tag value is stored once, and a series is kept as a short list of ids while counting. Measured on one machine: a DogStatsD sample of 900,000 unique series (64 MB) uses about 370 MB RSS and 10 seconds, about 6 times the input; before this change the same sample took about 1.3 GB and 24 seconds. A much larger sample needs memory and time accordingly.
- A Prometheus histogram or summary series that lands on the same name and kind as another sample counts once. The same tags are one combination. `latency_count` from a histogram `latency` and a counter named `latency.count` with the same labels are one metric.
- A report that cannot be written exits 2 (`could not write the report: REASON`). That is not success, and it does not panic. `--help` and `--version` that cannot be written exit 3. Control characters in currency, source, date, and in a metric name on an error line are printed as `\xNN`.
- Only count accepts --explain, --plan, --hosts, --allow-tags, --prices and --unbounded-threshold. On `diff` they exit 3 (`option does not apply to diff: <option>`), before samples or the price file are read.
- --hosts only works together with --plan. --untyped-as only works together with --format prometheus.
- There is no `--show-values` flag. The output shows tag keys and counts, not the tag values.
- A price is read as an exact decimal from the JSON number, not as a binary floating-point value. A price has at most 12 digits after the decimal point (`MAX_SCALE`) and a numerator of at most 10^18 (`MAX_NUMERATOR`). Trailing zeros in the fractional part are dropped before that limit. A price outside it makes the price file invalid (exit 2, message "field is too large").
- A JSON key repeated in the same object is rejected (exit 2). A JSONL value says `invalid jsonl line N: duplicate key`. A price file says `price file has a duplicate key`. The message does not include the raw text. A `\uXXXX` escape that decodes to the same key counts. The same name inside a nested value does not.

## Support

dd-metric-bill is free to use. If it saves you money, you can support the work via [Ko-fi](https://ko-fi.com/pepai).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Copyright 2026 PEPAI.

Release archives include THIRD-PARTY-LICENSES.html with the licenses of the third-party crates in the binary.

## Contact

PEPAI, pep_ai@icloud.com

Report bugs and ideas via [Issues](https://github.com/qwpp7g79y4-maker/dd-metric-bill/issues); report security problems privately as described in [SECURITY.md](SECURITY.md).

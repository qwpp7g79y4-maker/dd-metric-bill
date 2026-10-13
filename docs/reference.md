# dd-metric-bill reference

English · [Nederlands](reference.nl.md)

This page has the complete details. For an overview, see the [README](../README.md).

The text the binary prints is English. A decimal point is `.` (for example `0.10` and `1.5`). A comma in a number is not accepted.

## Options

Run `dd-metric-bill --help` or `dd-metric-bill count --help` for the built-in help. Options that do not apply to `diff` are rejected with exit 3 before any sample or price file is read. See [Exit codes and usage errors](#exit-codes-and-usage-errors).

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
| `--help` | everywhere | Show help and exit 0. |
| `--version` | top level | Show the version (`dd-metric-bill 0.1.0`) and exit 0. |

Only count accepts --explain, --plan, --hosts, --allow-tags, --prices and --unbounded-threshold. --hosts only works together with --plan. --untyped-as only works together with --format prometheus.

There is no `--show-values` flag. The output shows tag keys and counts, not the tag values.

## Price file

Prices come only from a JSON file you pass with `--prices`. The tool does not fetch a price and does not check the numbers against a Datadog page. The repository includes `prices/datadog-example.json` as an example of the shape:

```json
{
  "currency": "USD",
  "source": "https://docs.datadoghq.com/account_management/billing/custom_metrics",
  "date": "2026-10-07",
  "ingested_per_100": 0.10
}
```

Required fields: `currency`, `source`, `date`, and `ingested_per_100`. `indexed_per_100` may be absent. Any other field makes the file invalid. `currency`, `source`, and `date` are text and must not be empty after trimming. A price is a JSON number. A comma (`0,10`) is not valid JSON.

A price is read as an exact decimal from the JSON number, not as a binary floating-point value. A price has at most 12 digits after the decimal point (`MAX_SCALE`) and a numerator of at most 10^18 (`MAX_NUMERATOR`). Trailing zeros in the fractional part are dropped before that limit. A price outside it makes the price file invalid (exit 2, message "field is too large"). A negative price is rejected. The amount is whole-number arithmetic: metrics above the allotment, times the price per 100, divided by 100. The decimal point is `.`, and trailing zeros are not printed.

Without `--plan` the file still prints currency, source, and date, and there is no cost line. With `--plan`, each configured volume is priced against the same allotment. Ingested is priced only when `--allow-tags` is set. Indexed is priced only when `indexed_per_100` is present. Usage below the allotment prints a cost of 0.

## How it counts

A custom metric is one combination of metric name and tag values, including `host`. Tag order does not matter. The same combination on another line counts once. The submitted value does not count. Datadog's page says `Reordering tag values doesn't add uniqueness` (https://docs.datadoghq.com/account_management/billing/custom_metrics, read 7 October 2026).

- D3 COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).
- D4 HISTOGRAM: per unique tag combination, the number of agent aggregates. Default 5: `max`, `median`, `avg`, `count` (histogram_aggregates) plus `95percentile` (histogram_percentiles). Configurable with `--histogram-aggregates` and `--histogram-percentiles` (B1, tab Histogram).
- D5 DISTRIBUTION: 5 per unique tag combination (`count`, `sum`, `min`, `max`, `avg`); with percentiles (`p50`, `p75`, `p90`, `p95`, `p99`), 10 per combination (B1, tab Distribution).
- D10 Prometheus histogram and summary (Latest Version): _count and _sum become count (name.count and name.sum, without le or quantile), _bucket becomes count (name.bucket, with le) and quantiles become gauge (name.quantile, with quantile). 1 per unique tag combination (B2).
- D11 Prometheus without a type: a metric with TYPE untyped or without a TYPE line counts as 0 and is shown as skipped (B3). A _count, _sum or _bucket name whose base is a counter or gauge is a separate metric without a type (D11).
- D12 OpenMetrics counter: a _total sample of a counter family counts as count on name.count, 1 per label combination (B3).

With `--explain`, the rule text is that sentence plus `Configured factor: N.` for D4 and D5. The source URL is B1 for D3, D4, and D5 (https://docs.datadoghq.com/account_management/billing/custom_metrics), B2 for D10 (https://docs.datadoghq.com/integrations/guide/prometheus-metrics/), and B3 for D11 and D12 (https://docs.datadoghq.com/integrations/openmetrics/).

DogStatsD types: `c` count, `g` gauge, `h` histogram, `d` distribution, `ms` histogram, `s` gauge. A sample rate (`|@rate`) must be above 0 and at most 1. It does not change the count.

**Allowlist.** `--allow-tags` keeps, for the indexed count, only the named keys. Ingested is the count over every tag. Both use the same factor. The allowlist applies to every metric at once. An empty list keeps no tags.

**Allotment.** Pro is 100 per host, Enterprise is 200 per host (B1, Allocation). Hosts come from `--hosts` or from the distinct `host` values in the sample. Without an allowlist the usage is the total, which is indexed, and the text says `usage: N (total, indexed)`. With an allowlist, indexed and ingested are each set against the allotment. Surplus is 0 when usage is not above the allotment.

**Tags.** Per key: how many distinct values it has, and how many series disappear when that key is removed (`added`). `--top N` keeps the N keys with the largest `added`. Keys sort by `added`, then by number of values, then by name.

**Unbounded.** A key is unbounded when it has more distinct values than `--unbounded-threshold`, or when a value matches a pattern: a UUID (`8-4-4-4-12` hex), a hex id of at least 16 hex digits with at least one letter a-f, a numeric id of at least 6 digits, or a pod name `name-hash-hash` whose hash parts are at least 5 alphanumeric characters. Reasons in the output are `threshold`, `pattern uuid`, `pattern hex-id`, `pattern numeric-id`, and `pattern pod-name`. The values themselves are not printed. Status codes such as `200` are shorter than 6 digits, so they are not a numeric id.

**Diff.** Per metric name and kind: old count, new count, and delta. Metrics only in the new sample are listed under `new:`. Metrics only in the old sample are listed under `removed:`. Tag keys show old and new values and added series. `--budget` compares against the new total.

**Overflow.** Nothing panics on user input or on a failed write. The messages name the cause and use `1 combination` or `N combinations`:

- `allotment does not fit: the number of hosts times the metrics per host is too large`
- `count does not fit: the sum of --histogram-aggregates A and --histogram-percentiles P is too large`
- `count does not fit: metric NAME: factor F times N combinations is too large`
- `count does not fit: metric NAME: factor F times N combinations makes the total too large`

The sum of the histogram options is an error only when a histogram in the sample uses that factor. A sample with no histogram still counts when those flags are huge. Exit code 3, prefixed with `usage error: `.

Every text report ends with `estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)`.

## Input formats

**DogStatsD.** `name:value|type`, then optional `|@rate` and `|#tag:value,tag` in either order. A tag may have no value. Empty lines are skipped. A value that is not a finite number, including `NaN` and `Inf`, is invalid (exit 2). A very large finite value still counts: the value is not part of the bill.

**JSON lines.** One object per line: `metric` (text), `type` (`count`, `gauge`, `histogram`, or `distribution`), and `tags` (a list of `key:value` or `key`). Other fields are ignored. Empty lines are ignored.

**Prometheus text.** `# TYPE` sets the family. `counter` and `gauge` count as D3. `histogram` and `summary` follow D10, not the DogStatsD factor of 5. `# TYPE untyped`, or a sample with no `# TYPE`, counts as 0 and is shown as `skipped` (D11), unless `--untyped-as` is set. A `_count`, `_sum`, or `_bucket` name whose base is a counter or gauge is that separate untyped metric, not a histogram. Unknown types are an error. `# HELP` lines are ignored.

A histogram or summary series that maps to the same name and kind as another sample counts once. The same tags are one combination. `latency_count` under `# TYPE latency histogram` and a counter `# TYPE latency.count counter` with the same labels are one count metric.

A `_total` sample of a counter family counts as `name.count` (D12). The OpenMetrics page says: "counter names ending in `_total` must be specified without the `_total` suffix. For example, to collect `promhttp_metric_handler_requests_total`, specify the metric name `promhttp_metric_handler_requests`. This submits to Datadog the metric name appended with `.count`, `promhttp_metric_handler_requests.count`." (https://docs.datadoghq.com/integrations/openmetrics/, read 7 October 2026). `# TYPE a_total counter` with a sample named `a_total` stays the metric `a_total` (D3), because the type name matches the sample name exactly. A `_created` sample of a counter family does not count. What Datadog does with `_created` is not verified.

A metric name may contain a dot, without quotes. https://prometheus.io/docs/concepts/data_model/ (read 9 October 2026) says "Metric names MAY use any UTF-8 characters." and "Metric names SHOULD match the regex `[a-zA-Z_:][a-zA-Z0-9_:]*` for the best experience and compatibility (see the warning below). Metric names outside of that set will require quoting e.g. when used in PromQL (see the UTF-8 guide)." The tool accepts a dot without quotes because Datadog names such as `request.Latency` appear in source B1. The characters the parser accepts in a name are letters, digits, `_`, `:`, and `.`.

A label list may end with a comma. The exposition format writes `label_pairs = label_pair { "," label_pair } [ "," ]` (https://github.com/prometheus/docs/blob/main/docs/instrumenting/exposition_formats.md, read 8 October 2026). Spaces around `=` are accepted, for example `a{host = "x"} 1`. The EBNF on that page (read 9 October 2026) writes the label name, then `=`, then a quoted value, with no spaces. The same page says tokens may be separated by spaces or tabs. This tool accepts the spaces: milder than the grammar, never stricter.

`+Inf` is a valid Prometheus value. A timestamp, when present, is an optional integer. A UTF-8 BOM at the start of a DogStatsD, JSONL, or Prometheus file is skipped. A BOM later in the file stays part of the text. Control characters in a metric name, a tag key, a price file's currency, source, or date, and a metric name in an error message are printed as `\xNN` in text and on stderr so they do not drive the terminal. JSON keeps the raw string.

Number parsing goes through `serde_json` 1.0.151, which depends on `zmij` 1.0.23 by David Tolnay (MIT). That library has no network, file, or process access. Its build script runs `rustc --version` while compiling.

## JSON

`schema_version` is 1. `tool` is `dd-metric-bill`. `estimate` is the estimate line. `source` is the custom-metrics billing URL. For `count`, `inputs` is the list of paths and `total` is a number. For `diff`, `inputs` is `{"old","new"}` and `total` is `{"old","new","delta"}`.

Each metric has `name`, `type`, `combinations`, `count`, `rule`, and `rule_text`. With `--allow-tags`, metrics also have `ingested` and `indexed`, and the document has those totals. `tags` lists keys with `values` and `added`. `unbounded` lists keys with `reasons`. `allotment` is `null` without `--plan`. Without an allowlist it has `usage`, `basis` (`total (indexed)`), and `surplus`. With an allowlist it has `indexed`, `indexed_surplus`, `ingested`, and `ingested_surplus`, and no `basis` key. `costs` is present only with `--prices`: `currency`, `source`, `date`, and `lines` of `kind`, `above`, and `amount`. `diff` leaves `allotment` null and `unbounded` empty, and omits `ingested`, `indexed`, and `costs`.

`--explain` does not change the JSON: `rule_text` and `source` are always there. Tag values are not included.

## Exit codes and usage errors

| Code | Meaning |
| --- | --- |
| 0 | The report was printed. With `--budget`, the total is not above N. |
| 1 | `--budget` exceeded. Stdout still has the report. Stderr is `budget exceeded: TOTAL > LIMIT`. |
| 2 | The sample or the price file cannot be read or is invalid, or the report cannot be written. |
| 3 | Usage error. Stdout is empty. `--help` and `--version` that cannot be written also exit 3. |

When stdout cannot take the report, stderr is `could not write the report: REASON` and the exit code is 2. `--help` and `--version` that cannot be written exit 3, and stderr is `could not write the message: REASON`. A failed write does not panic.

A sample and a price file must be regular files. `metadata` follows symlinks. A directory, FIFO, device, or other non-regular path is not opened, so a FIFO with no writer does not hang. The message is `cannot read sample: PATH: REASON` or `cannot read price file: PATH: REASON`, and `REASON` includes `not a regular file` when the path is not a regular file.

A price file that is not valid JSON, not an object, or has a bad field prints `invalid price file: PATH: DETAIL`. `DETAIL` is one of: `not valid json`, `not an object`, `missing field: FIELD`, `empty field: FIELD`, `field is not text: FIELD`, `field is not a number: FIELD`, `field is negative: FIELD`, `field is too large: FIELD`, `unknown field`.

A bad sample names the format and the 1-based line number, never the line itself:

- `invalid dogstatsd line N: shape is wrong`
- `invalid dogstatsd line N: name is missing`
- `invalid dogstatsd line N: value is missing or invalid`
- `invalid dogstatsd line N: type is missing or unknown`
- `invalid dogstatsd line N: sample rate is missing or invalid`
- `invalid dogstatsd line N: tag is missing or invalid`
- `invalid dogstatsd line N: unknown field`
- `invalid jsonl line N: not valid json`
- `invalid jsonl line N: shape is wrong`
- `invalid jsonl line N: name is missing`
- `invalid jsonl line N: type is missing or unknown`
- `invalid jsonl line N: tag is missing or invalid`
- `invalid prometheus line N: shape is wrong`
- `invalid prometheus line N: name is missing or invalid`
- `invalid prometheus line N: type is missing or unknown`
- `invalid prometheus line N: type is duplicated`
- `invalid prometheus line N: label is missing or invalid`
- `invalid prometheus line N: value is missing or invalid`
- `invalid prometheus line N: histogram sample is missing _bucket, _sum or _count`

On `diff`, forbidden options are checked in this order: `--explain`, `--plan`, `--hosts`, `--allow-tags`, `--prices`, `--unbounded-threshold`. The first one wins, so `--explain` wins over a later flag. `--json` does not change the message. An invalid value is reported by the argument parser before that check.

Every row below was run against the binary, and a test keeps it that way. A row with exit 3 has empty stdout. The message is the complete stderr; the binary adds a newline. `\n` in the last column is a newline inside the message.

| Option | Applies to | Case | Wrong use | Exit | Message |
| --- | --- | --- | --- | --- | --- |
| `--format` | count | Invalid value. | `count --format nope tests/fixtures/empty.dogstatsd` | 3 | `error: invalid value 'nope' for '--format <FORMAT>'\n  [possible values: dogstatsd, jsonl, prometheus]\n\nFor more information, try '--help'.\n` |
| `--plan` | count | Invalid value. | `count --plan maybe tests/fixtures/empty.dogstatsd` | 3 | `error: invalid value 'maybe' for '--plan <PLAN>'\n  [possible values: pro, enterprise]\n\nFor more information, try '--help'.\n` |
| `--untyped-as` | count | Invalid value. | `count --untyped-as nope tests/fixtures/empty.dogstatsd` | 3 | `error: invalid value 'nope' for '--untyped-as <KIND>'\n  [possible values: gauge, count]\n\nFor more information, try '--help'.\n` |
| `--histogram-aggregates` | count | Not an integer. | `count --histogram-aggregates nope tests/fixtures/empty.dogstatsd` | 3 | `error: invalid value 'nope' for '--histogram-aggregates <N>': invalid digit found in string\n\nFor more information, try '--help'.\n` |
| `--top` | count | Not an integer. | `count --top nope tests/fixtures/empty.dogstatsd` | 3 | `error: invalid value 'nope' for '--top <N>': invalid digit found in string\n\nFor more information, try '--help'.\n` |
| `--budget` | count | Not an integer. | `count --budget nope tests/fixtures/empty.dogstatsd` | 3 | `error: invalid value 'nope' for '--budget <N>': invalid digit found in string\n\nFor more information, try '--help'.\n` |
| `--hosts` | count | Without `--plan`. | `count --hosts 1 tests/fixtures/empty.dogstatsd` | 3 | `--hosts only works together with --plan` |
| `--untyped-as` | count | Without prometheus. | `count --untyped-as gauge tests/fixtures/empty.dogstatsd` | 3 | `--untyped-as only works together with --format prometheus` |
| `--explain` | diff | Not for diff. | `diff --explain tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --explain` |
| `--explain` | diff | `--json` does not change the message. | `diff --json --explain tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --explain` |
| `--plan` | diff | Checked before `--hosts`. | `diff --plan pro --hosts 1 tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --plan` |
| `--hosts` | diff | Not for diff. | `diff --hosts 1 tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --hosts` |
| `--allow-tags` | diff | Not for diff. | `diff --allow-tags host tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --allow-tags` |
| `--prices` | diff | Rejected before the price file is read. | `diff --prices prices/datadog-example.json tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --prices` |
| `--unbounded-threshold` | diff | Set on the command line, even at the default. | `diff --unbounded-threshold 50 tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --unbounded-threshold` |
| `--help` | top level | Unknown option. | `--nope` | 3 | `error: unexpected argument '--nope' found\n\n  tip: a similar argument exists: '--top'\n\nUsage: dd-metric-bill --top <N> <COMMAND>\n\nFor more information, try '--help'.\n` |
| `diff` | diff | The new sample is missing. | `diff tests/fixtures/diff-old.dogstatsd` | 3 | `error: the following required arguments were not provided:\n  <NEW>\n\nUsage: dd-metric-bill diff <OLD> <NEW>\n\nFor more information, try '--help'.\n` |

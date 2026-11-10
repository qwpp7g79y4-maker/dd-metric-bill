# Datadog Metric Bill

This plugin helps Claude use the `dd-metric-bill` command-line tool. The tool estimates how many billable Datadog custom metrics a local sample produces. It works offline and does not call the Datadog API.

## Use it

Install the tool with Rust 1.85 or newer: `cargo install --locked dd-metric-bill`. Give Claude a DogStatsD, JSONL, or Prometheus sample, and ask how many custom metrics it would bill, or how two samples differ. Costs appear only when you pass your own price file with `--prices`.

## Data

The tool reads only the sample files and optional price file you pass. It does not use a Datadog API key and does not send telemetry. The estimate is one snapshot; Datadog bills the hourly average over the month. This plugin stores nothing and sends nothing to any service. Contracts with Metric Name pricing are not supported.

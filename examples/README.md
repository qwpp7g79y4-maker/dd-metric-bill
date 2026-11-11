# Examples

Fictional samples, written by hand. Nothing here was captured from a live agent or from the network.

## page.dogstatsd

One DogStatsD line, the default format named by `dd-metric-bill --help`. `page.views` is a count. The tags `env:prod` and `host:web-1` are made up.

From the repository root:

```
dd-metric-bill count examples/page.dogstatsd
```

Exit code: 0

## webshop.dogstatsd and api-gateway.prom

Two fictional samples, written by hand for a small web shop and an API gateway. Names, tags, hosts, and numbers are made up; there is no customer data, key, or token in them. Each has the exact output of the binary next to it, and `tests/examples.rs` compares the two.

From the repository root, the output is in `webshop.expected.txt`:

```
dd-metric-bill count examples/webshop.dogstatsd
```

Exit code: 0

The output is in `api-gateway.expected.txt`:

```
dd-metric-bill count --format prometheus examples/api-gateway.prom
```

Exit code: 0

# Examples

Fictional samples, written by hand. Nothing here was captured from a live agent or from the network.

## page.dogstatsd

One DogStatsD line, the default format named by `dd-metric-bill --help`. `page.views` is a count. The tags `env:prod` and `host:web-1` are made up.

From the repository root:

```
dd-metric-bill count examples/page.dogstatsd
```

Exit code: 0

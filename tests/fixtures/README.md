# Fixtures

Every file in this directory is synthetic: written by hand for a test, not taken from a Datadog account, an agent, or a production system. Names, tag values, and numbers are made up.

## b1-count.dogstatsd

Synthetic DogStatsD sample after the worked example of Datadog custom metrics (source B1). Metric `request.Latency`, type count (`|c`). Tags: `host` A and B, `endpoint` X and Y, `status` 200 and 400. That is four unique combinations. The fifth line repeats the first combination (`host` A, `endpoint` X, `status` 200) with another made-up number, so a duplicate line counts once.

## b1-distribution.dogstatsd

Synthetic. The same five lines as `b1-count.dogstatsd`, with the type changed to `|d` (distribution).

## b1-histogram.dogstatsd

Synthetic. The same five lines as `b1-count.dogstatsd`, with the type changed to `|h` (histogram).

## diff-new.dogstatsd

Synthetic new side of a comparison with `diff-old.dogstatsd`. `page.views` keeps `host` and `env` prod and gains the tag `region` (eu on host a and b, us on host a): three combinations. `checkout` is added, one count with only `env` prod. `queue.depth` from the old sample is not here.

## diff-old.dogstatsd

Synthetic old side of that comparison. Two count lines `page.views` on host a and host b with `env` prod, plus one gauge `queue.depth` on host a. Together three custom metrics. There is no `region` and no `checkout`.

## empty.dogstatsd

Synthetic. The file has no metric line, only one empty line (a newline). It is an empty sample: total 0.

## invalid-line.dogstatsd

Synthetic. One line that is not a DogStatsD sample, so `count` exits 2.

## invalid-line.jsonl

Synthetic. One broken JSON line, so `count --format jsonl` exits 2.

## invalid-line.prom

Synthetic. One Prometheus line without a value, so `count --format prometheus` exits 2.

## mixed.dogstatsd

Synthetic, with every DogStatsD type in one sample. `page.views` is a count on host web-1 and web-2 with `env` prod; the third line repeats web-1 with the tags in reverse order. `queue.depth` is a gauge, twice on the same host web-1. `rpc.latency` is a histogram: one `|h` line and one `|ms` line (`ms` counts as histogram), on web-1 and web-2, tag `endpoint` checkout. `payload.bytes` is a distribution (`|d`) on web-1. `users.online` is a set (`|s`) with the made-up member alice; a set counts as gauge.

## prices-comma.json

Synthetic price file. The ingested price is written with a comma (`0,10`), which is not valid JSON.

## prices-invalid.txt

Synthetic. The file is the text `not-json`, not a JSON object.

## prices-negative.json

Synthetic price file with a negative `ingested_per_100`.

## prices-too-large.json

Synthetic price file. `ingested_per_100` has more than 12 digits after the decimal point.

## two-histograms.dogstatsd

Synthetic. Two histogram metrics, `a` and `b`, each with one tag combination. Used to show the overflow message when the two counts do not fit in one total.

## user-id.dogstatsd

Synthetic, so the key `user_id` adds the most series. Four count lines `checkout` with `env` prod. Three lines have `endpoint` pay and `user_id` u1, u2, and u3. The fourth has `endpoint` cart and u3 again. Without `user_id` two combinations remain (pay and cart); with `user_id` there are four, so that key adds two.

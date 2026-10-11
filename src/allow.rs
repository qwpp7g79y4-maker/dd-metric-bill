use std::collections::BTreeSet;

use crate::count::{count_by_metric, CountError};
use crate::parse::{Kind, Sample, Tag};

/// Allowlist count for one metric name and kind (D6).
///
/// `ingested` is present once an allowlist is set: the count over all
/// tags. `indexed` is the count over only the allowed keys, or, without an
/// allowlist, the ordinary count (D1-D5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricAllow {
    pub name: String,
    pub kind: Kind,
    pub ingested: Option<u64>,
    pub indexed: u64,
}

/// Ingested and indexed over a sample (D6).
///
/// Without an allowlist, `ingested` is absent and `indexed` equals the total.
/// Only metrics with an allowlist count toward ingested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowReport {
    pub metrics: Vec<MetricAllow>,
    pub ingested: Option<u64>,
    pub indexed: u64,
}

/// Keys from `--allow-tags`, comma-separated.
///
/// Spaces around a key are dropped. Empty pieces are dropped too.
#[must_use]
pub fn parse_allow_tags(list: &str) -> BTreeSet<String> {
    list.split(',')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .collect()
}

/// Simulates Metrics without Limits (D6).
///
/// `allow_tags` is `None` without an allowlist. Then `indexed` is the count
/// from D1-D5 and `ingested` is absent. With an allowlist, `ingested` is the
/// count over all tags and `indexed` is the count over only those keys. An
/// empty set keeps no tags. The histogram and distribution factor (D4, D5)
/// applies to both counts.
///
/// # Errors
///
/// [`CountError`] when the count does not fit in `u64`.
pub fn simulate(
    samples: &[Sample],
    allow_tags: Option<&BTreeSet<String>>,
    histogram_aggregates: u64,
    histogram_percentiles: u64,
    percentiles: bool,
) -> Result<AllowReport, CountError> {
    let Some(allow_tags) = allow_tags else {
        return only_indexed(
            samples,
            histogram_aggregates,
            histogram_percentiles,
            percentiles,
        );
    };
    let ingested = count_by_metric(
        samples,
        histogram_aggregates,
        histogram_percentiles,
        percentiles,
    )?;
    let indexed = count_by_metric(
        &only_allowed(samples, allow_tags),
        histogram_aggregates,
        histogram_percentiles,
        percentiles,
    )?;
    assert_eq!(
        ingested.metrics.len(),
        indexed.metrics.len(),
        "filtered tags keep the same metrics"
    );
    let metrics = ingested
        .metrics
        .iter()
        .zip(indexed.metrics.iter())
        .map(|(vol, part)| {
            assert_eq!(vol.name, part.name, "filtered tags keep the same metrics");
            assert_eq!(vol.kind, part.kind, "filtered tags keep the same metrics");
            MetricAllow {
                name: vol.name.clone(),
                kind: vol.kind,
                ingested: Some(vol.count),
                indexed: part.count,
            }
        })
        .collect();
    Ok(AllowReport {
        metrics,
        ingested: Some(ingested.total),
        indexed: indexed.total,
    })
}

fn only_indexed(
    samples: &[Sample],
    histogram_aggregates: u64,
    histogram_percentiles: u64,
    percentiles: bool,
) -> Result<AllowReport, CountError> {
    let report = count_by_metric(
        samples,
        histogram_aggregates,
        histogram_percentiles,
        percentiles,
    )?;
    let metrics = report
        .metrics
        .iter()
        .map(|metric| MetricAllow {
            name: metric.name.clone(),
            kind: metric.kind,
            ingested: None,
            indexed: metric.count,
        })
        .collect();
    Ok(AllowReport {
        metrics,
        ingested: None,
        indexed: report.total,
    })
}

fn only_allowed(samples: &[Sample], allow_tags: &BTreeSet<String>) -> Vec<Sample> {
    samples
        .iter()
        .map(|sample| Sample {
            name: sample.name.clone(),
            kind: sample.kind,
            tags: sample
                .tags
                .iter()
                .filter(|tag| allow_tags.contains(&tag.key))
                .cloned()
                .collect::<Vec<Tag>>(),
            d10: sample.d10,
            counter_total: sample.counter_total,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse_allow_tags, simulate};
    use crate::count::{
        count_by_metric, DEFAULT_HISTOGRAM_AGGREGATES, DEFAULT_HISTOGRAM_PERCENTILES,
    };
    use crate::parse::{parse_dogstatsd, Kind};

    fn b1_request_latency(kind: &str) -> String {
        format!(
            "\
request.Latency:10|{kind}|#host:A,endpoint:X,status:200
request.Latency:12|{kind}|#host:B,endpoint:X,status:200
request.Latency:40|{kind}|#host:B,endpoint:X,status:400
request.Latency:8|{kind}|#host:B,endpoint:Y,status:200
request.Latency:11|{kind}|#host:A,endpoint:X,status:200
"
        )
    }

    fn allow(list: &str) -> std::collections::BTreeSet<String> {
        parse_allow_tags(list)
    }

    #[test]
    fn b1_count_4_ingested_3_indexed() {
        let samples = parse_dogstatsd(&b1_request_latency("c")).expect("synthetic sample");
        let keys = allow("endpoint,status");
        let out = simulate(
            &samples,
            Some(&keys),
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        assert_eq!(out.ingested, Some(4));
        assert_eq!(out.indexed, 3);
        assert_eq!(out.metrics.len(), 1);
        assert_eq!(out.metrics[0].name, "request.Latency");
        assert_eq!(out.metrics[0].kind, Kind::Count);
        assert_eq!(out.metrics[0].ingested, Some(4));
        assert_eq!(out.metrics[0].indexed, 3);
    }

    #[test]
    fn b1_distribution_20_ingested_15_indexed() {
        let samples = parse_dogstatsd(&b1_request_latency("d")).expect("synthetic sample");
        let keys = allow("endpoint,status");
        let out = simulate(
            &samples,
            Some(&keys),
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        assert_eq!(out.ingested, Some(20));
        assert_eq!(out.indexed, 15);
        assert_eq!(out.metrics.len(), 1);
        assert_eq!(out.metrics[0].kind, Kind::Distribution);
        assert_eq!(out.metrics[0].ingested, Some(20));
        assert_eq!(out.metrics[0].indexed, 15);
    }

    #[test]
    fn without_allowlist_only_indexed_equals_total() {
        let samples = parse_dogstatsd(&b1_request_latency("c")).expect("synthetic sample");
        let counted = count_by_metric(
            &samples,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        let out = simulate(
            &samples,
            None,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        assert_eq!(out.ingested, None);
        assert_eq!(out.indexed, counted.total);
        assert_eq!(out.indexed, 4);
        assert_eq!(out.metrics.len(), counted.metrics.len());
        assert!(out.metrics.iter().all(|metric| metric.ingested.is_none()));
        assert_eq!(out.metrics[0].indexed, counted.metrics[0].count);
        assert_eq!(out.metrics[0].indexed, out.indexed);
    }

    #[test]
    fn b1_histogram_20_ingested_15_indexed() {
        let samples = parse_dogstatsd(&b1_request_latency("h")).expect("synthetic sample");
        let keys = allow("endpoint,status");
        let out = simulate(
            &samples,
            Some(&keys),
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        assert_eq!(out.ingested, Some(20));
        assert_eq!(out.indexed, 15);
        assert_eq!(out.metrics[0].kind, Kind::Histogram);
    }

    #[test]
    fn per_metric_sums_separately() {
        let input = "\
page.views:1|c|#host:a,env:prod
page.views:1|c|#host:b,env:prod
temp:1|g|#city:x
temp:1|g|#city:y
";
        let samples = parse_dogstatsd(input).expect("synthetic sample");
        let keys = allow("env");
        let out = simulate(
            &samples,
            Some(&keys),
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        assert_eq!(out.ingested, Some(4));
        assert_eq!(out.indexed, 2);
        assert_eq!(out.metrics[0].name, "page.views");
        assert_eq!(out.metrics[0].kind, Kind::Count);
        assert_eq!(out.metrics[0].ingested, Some(2));
        assert_eq!(out.metrics[0].indexed, 1);
        assert_eq!(out.metrics[1].name, "temp");
        assert_eq!(out.metrics[1].kind, Kind::Gauge);
        assert_eq!(out.metrics[1].ingested, Some(2));
        assert_eq!(out.metrics[1].indexed, 1);
    }

    #[test]
    fn parse_trims_and_skips_empty_pieces() {
        let keys = parse_allow_tags(" endpoint, status, ,endpoint");
        assert_eq!(
            keys.into_iter().collect::<Vec<_>>(),
            vec!["endpoint".to_string(), "status".to_string()]
        );
    }
}

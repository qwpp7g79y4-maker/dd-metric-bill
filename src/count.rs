use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};

use crate::parse::{Kind, Sample};

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct TagPair {
    key: String,
    value: Option<String>,
}

/// Default `histogram_aggregates`: `max`, `median`, `avg`, `count` (D4).
pub const DEFAULT_HISTOGRAM_AGGREGATES: u64 = 4;

/// Default `histogram_percentiles`: `95percentile` (D4).
pub const DEFAULT_HISTOGRAM_PERCENTILES: u64 = 1;

/// Overflow from `--hosts` or from the histogram factor.
///
/// User input must not panic. The CLI turns this into exit 3.
/// [`CountError::Sum`] happens only when a histogram actually uses the factor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CountError {
    /// Hosts times the metrics per host does not fit in `u64`.
    Allotment,
    /// `--histogram-aggregates` plus `--histogram-percentiles` does not fit in `u64`.
    Sum { aggregates: u64, percentiles: u64 },
    /// The factor times the number of combinations does not fit in `u64`.
    Product {
        metric: String,
        factor: u64,
        combinations: u64,
    },
    /// The sum of the metric counts does not fit in `u64`.
    Total {
        metric: String,
        factor: u64,
        combinations: u64,
    },
}

impl fmt::Display for CountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CountError::Allotment => f.write_str(
                "allotment does not fit: the number of hosts times the metrics per host is too large",
            ),
            CountError::Sum {
                aggregates,
                percentiles,
            } => write!(
                f,
                "count does not fit: the sum of --histogram-aggregates {aggregates} and --histogram-percentiles {percentiles} is too large"
            ),
            CountError::Product {
                metric,
                factor,
                combinations,
            } => {
                let metric = visible_text(metric);
                write!(
                    f,
                    "count does not fit: metric {metric}: factor {factor} times {combinations} {word} is too large",
                    word = combinations_word(*combinations),
                )
            }
            CountError::Total {
                metric,
                factor,
                combinations,
            } => {
                let metric = visible_text(metric);
                write!(
                    f,
                    "count does not fit: metric {metric}: factor {factor} times {combinations} {word} makes the total too large",
                    word = combinations_word(*combinations),
                )
            }
        }
    }
}

fn combinations_word(n: u64) -> &'static str {
    if n == 1 {
        "combination"
    } else {
        "combinations"
    }
}

/// Control characters as `\xNN`, so a metric name or a price field does not drive the terminal.
/// JSON does not use this function: the raw string stays there.
pub(crate) fn visible_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_control() {
            write!(out, "\\x{:02x}", u32::from(ch)).expect("writing to a String does not fail");
        } else {
            out.push(ch);
        }
    }
    out
}

impl std::error::Error for CountError {}

/// Source B1: Custom Metrics Billing.
pub const CUSTOM_METRICS_SOURCE: &str =
    "https://docs.datadoghq.com/account_management/billing/custom_metrics";

/// Source B2: Mapping Prometheus Metrics to Datadog Metrics, tab Latest Version.
pub const PROMETHEUS_MAPPING_SOURCE: &str =
    "https://docs.datadoghq.com/integrations/guide/prometheus-metrics/";

/// Source B3: Datadog, OpenMetrics, section Missing untyped metrics.
pub const OPENMETRICS_SOURCE: &str = "https://docs.datadoghq.com/integrations/openmetrics/";

/// Distribution without percentiles: `count`, `sum`, `min`, `max`, `avg` (D5).
const DISTRIBUTION_WITHOUT_PERCENTILES: u64 = 5;

/// Distribution with `--percentiles`: the five base metrics plus `p50`, `p75`, `p90`, `p95`, `p99` (D5).
const DISTRIBUTION_WITH_PERCENTILES: u64 = 10;

/// D4 billing rule text. The configured factor is appended.
const D4_TEXT: &str = "HISTOGRAM: per unique tag combination, the number of agent aggregates. Default 5: `max`, `median`, `avg`, `count` (histogram_aggregates) plus `95percentile` (histogram_percentiles). Configurable with `--histogram-aggregates` and `--histogram-percentiles` (B1, tab Histogram).";

/// D5 billing rule text. The configured factor is appended.
const D5_TEXT: &str = "DISTRIBUTION: 5 per unique tag combination (`count`, `sum`, `min`, `max`, `avg`); with percentiles (`p50`, `p75`, `p90`, `p95`, `p99`), 10 per combination (B1, tab Distribution).";

/// Rule that sets the count for one metric, with a source URL (B1 or B2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub code: &'static str,
    pub text: String,
    pub source: &'static str,
}

/// Billable count of one metric name and kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricCount {
    pub name: String,
    pub kind: Kind,
    pub combinations: u64,
    pub count: u64,
    pub rule: Rule,
}

/// Count over a sample: per metric and the total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountReport {
    pub metrics: Vec<MetricCount>,
    pub total: u64,
}

/// Custom metrics per host on Pro (D7, B1 Allocation).
pub const PRO_PER_HOST: u64 = 100;

/// Custom metrics per host on Enterprise (D7, B1 Allocation).
pub const ENTERPRISE_PER_HOST: u64 = 200;

/// Plan for the allotment (D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Pro,
    Enterprise,
}

/// One volume against the allotment (D7). `surplus` is 0 below the allotment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgainstAllotment {
    pub usage: u64,
    pub surplus: u64,
}

/// Allotment against the usage in one sample (D6, D7).
///
/// Without an allowlist, `indexed` is the total (that is indexed) and `ingested`
/// is absent. With an allowlist, indexed and ingested are each listed separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allotment {
    pub plan: Plan,
    pub hosts: u64,
    pub allotment: u64,
    pub indexed: AgainstAllotment,
    pub ingested: Option<AgainstAllotment>,
}

struct Group {
    kind: Kind,
    combinations: BTreeSet<BTreeSet<TagPair>>,
    /// True only when every sample in the group is a Prometheus histogram or summary.
    all_d10: bool,
    /// True only when every sample in the group is an OpenMetrics counter `_total`.
    all_counter_total: bool,
}

/// Counts billable custom metrics for count and gauge (D1-D3).
///
/// A custom metric is a unique combination of name and tag values,
/// including the `host` tag. Tag order does not matter. The same
/// combination on multiple lines counts once; the submitted value does not.
/// Histogram and distribution follow D4 and D5 and are not in this count.
///
/// # Errors
///
/// [`CountError`] when a histogram in the sample uses the factor and that count does not fit in `u64`.
pub fn count_and_gauge(samples: &[Sample]) -> Result<u64, CountError> {
    sum_of(
        count_by_metric(
            samples,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )?,
        |metric| matches!(metric.kind, Kind::Count | Kind::Gauge),
    )
}

/// Counts histogram (D4) and distribution (D5) on unique tag combinations (D1, D2).
///
/// Count and gauge are skipped. Each unique combination of name and
/// tag values counts once, then multiplied:
/// histogram by `histogram_aggregates + histogram_percentiles` (default 4 + 1 = 5),
/// distribution by 5, or by 10 when `percentiles` (`--percentiles`) is on.
/// The submitted value does not count.
///
/// # Errors
///
/// [`CountError`] when the histogram factor, the product with the combinations, or the total does not fit in `u64`.
pub fn count_histogram_distribution(
    samples: &[Sample],
    histogram_aggregates: u64,
    histogram_percentiles: u64,
    percentiles: bool,
) -> Result<u64, CountError> {
    sum_of(
        count_by_metric(
            samples,
            histogram_aggregates,
            histogram_percentiles,
            percentiles,
        )?,
        |metric| matches!(metric.kind, Kind::Histogram | Kind::Distribution),
    )
}

/// Counts, per metric name and kind, the unique tag combinations and the billable count.
///
/// Count and gauge (D3) count 1 per combination. Histogram (D4) counts
/// `histogram_aggregates + histogram_percentiles` per combination.
/// Distribution (D5) counts 5, or 10 with `percentiles`.
/// A Prometheus histogram or summary (D10) counts 1 per combination, as count
/// or gauge, even when the histogram flags differ.
/// A `_total` sample of an OpenMetrics counter (D12) counts 1 per combination
/// on `name.count`.
/// A Prometheus metric without a type (D11) counts 0.
/// Samples that share a name and a kind are one metric. A histogram `_count`
/// mapped to `name.count` and a counter already named `name.count` therefore
/// count once: the same tags are one combination.
/// Each metric gets the rule code, the rule text, and the source URL.
/// Metrics are ordered by name, and for the same name by kind.
///
/// # Errors
///
/// [`CountError::Sum`] when a histogram in the sample uses the factor and
/// `--histogram-aggregates` plus `--histogram-percentiles` does not fit in `u64`.
/// [`CountError::Product`] when that factor times the number of combinations does not fit.
/// [`CountError::Total`] when the sum of the counts does not fit.
/// Count, gauge, distribution, D10, and D12 do not use that factor: extreme
/// histogram flags then do not produce an error.
pub fn count_by_metric(
    samples: &[Sample],
    histogram_aggregates: u64,
    histogram_percentiles: u64,
    percentiles: bool,
) -> Result<CountReport, CountError> {
    let mut groups: BTreeMap<(String, u8), Group> = BTreeMap::new();
    for sample in samples {
        let key = (sample.name.clone(), kind_ord(sample.kind));
        let group = groups.entry(key).or_insert_with(|| Group {
            kind: sample.kind,
            combinations: BTreeSet::new(),
            all_d10: true,
            all_counter_total: true,
        });
        // A mixed group is no longer a pure D10 or D12 series. The factor stays 1.
        group.all_d10 &= sample.d10;
        group.all_counter_total &= sample.counter_total;
        group.combinations.insert(tags_of(sample));
    }

    let mut metrics = Vec::with_capacity(groups.len());
    let mut total = 0u64;
    for ((name, _), group) in groups {
        let combinations =
            u64::try_from(group.combinations.len()).expect("number of combinations fits in u64");
        let (factor, rule) = factor_and_rule(
            group.kind,
            group.all_d10,
            group.all_counter_total,
            histogram_aggregates,
            histogram_percentiles,
            percentiles,
        )?;
        let count = scale(&name, combinations, factor)?;
        total = total.checked_add(count).ok_or_else(|| CountError::Total {
            metric: name.clone(),
            factor,
            combinations,
        })?;
        metrics.push(MetricCount {
            name,
            kind: group.kind,
            combinations,
            count,
            rule,
        });
    }
    Ok(CountReport { metrics, total })
}

/// Allotment: Pro 100 and Enterprise 200 custom metrics per host, summed (D7).
///
/// Without an allowlist, `indexed` is the total. `ingested` is set with an
/// allowlist; both volumes sit separately against the same allotment (D6, D7).
/// The surplus is the volume minus the allotment, or 0 when the volume is lower.
///
/// # Errors
///
/// [`CountError::Allotment`] when hosts times the metrics per host does not fit in `u64`.
pub fn allotment(
    plan: Plan,
    hosts: u64,
    indexed: u64,
    ingested: Option<u64>,
) -> Result<Allotment, CountError> {
    let per_host = match plan {
        Plan::Pro => PRO_PER_HOST,
        Plan::Enterprise => ENTERPRISE_PER_HOST,
    };
    let allotted = hosts.checked_mul(per_host).ok_or(CountError::Allotment)?;
    Ok(Allotment {
        plan,
        hosts,
        allotment: allotted,
        indexed: against(indexed, allotted),
        ingested: ingested.map(|usage| against(usage, allotted)),
    })
}

fn against(usage: u64, allotted: u64) -> AgainstAllotment {
    AgainstAllotment {
        usage,
        surplus: usage.saturating_sub(allotted),
    }
}

/// Unique values of the `host` tag. A tag without a value does not count (D7).
#[must_use]
pub fn unique_hosts(samples: &[Sample]) -> u64 {
    let hosts: BTreeSet<&str> = samples
        .iter()
        .flat_map(|sample| &sample.tags)
        .filter(|tag| tag.key == "host")
        .filter_map(|tag| tag.value.as_deref())
        .collect();
    u64::try_from(hosts.len()).expect("number of hosts fits in u64")
}

fn sum_of(report: CountReport, keep: impl Fn(&MetricCount) -> bool) -> Result<u64, CountError> {
    report
        .metrics
        .iter()
        .filter(|metric| keep(metric))
        .try_fold(0u64, |acc, metric| {
            acc.checked_add(metric.count)
                .ok_or_else(|| CountError::Total {
                    metric: metric.name.clone(),
                    factor: factor_of(metric),
                    combinations: metric.combinations,
                })
        })
}

fn kind_ord(kind: Kind) -> u8 {
    match kind {
        Kind::Count => 0,
        Kind::Gauge => 1,
        Kind::Histogram => 2,
        Kind::Distribution => 3,
        Kind::Untyped => 4,
    }
}

fn tags_of(sample: &Sample) -> BTreeSet<TagPair> {
    sample
        .tags
        .iter()
        .map(|tag| TagPair {
            key: tag.key.clone(),
            value: tag.value.clone(),
        })
        .collect()
}

fn factor_and_rule(
    kind: Kind,
    d10: bool,
    counter_total: bool,
    histogram_aggregates: u64,
    histogram_percentiles: u64,
    percentiles: bool,
) -> Result<(u64, Rule), CountError> {
    if counter_total {
        return Ok((1, rule_d12()));
    }
    if d10 {
        return Ok((1, rule_d10()));
    }
    match kind {
        Kind::Count | Kind::Gauge => Ok((1, rule_d3())),
        Kind::Histogram => {
            let factor = histogram_aggregates
                .checked_add(histogram_percentiles)
                .ok_or(CountError::Sum {
                    aggregates: histogram_aggregates,
                    percentiles: histogram_percentiles,
                })?;
            Ok((factor, rule_d4(factor)))
        }
        Kind::Distribution => {
            if percentiles {
                Ok((
                    DISTRIBUTION_WITH_PERCENTILES,
                    rule_d5(DISTRIBUTION_WITH_PERCENTILES),
                ))
            } else {
                Ok((
                    DISTRIBUTION_WITHOUT_PERCENTILES,
                    rule_d5(DISTRIBUTION_WITHOUT_PERCENTILES),
                ))
            }
        }
        Kind::Untyped => Ok((0, rule_d11())),
    }
}

fn rule_d3() -> Rule {
    Rule {
        code: "D3",
        text: "COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).".to_string(),
        source: CUSTOM_METRICS_SOURCE,
    }
}

fn with_factor(text: &str, factor: u64) -> String {
    format!("{text} Configured factor: {factor}.")
}

fn rule_d4(factor: u64) -> Rule {
    Rule {
        code: "D4",
        text: with_factor(D4_TEXT, factor),
        source: CUSTOM_METRICS_SOURCE,
    }
}

fn rule_d10() -> Rule {
    Rule {
        code: "D10",
        text: "Prometheus histogram and summary (Latest Version): _count and _sum become count (name.count and name.sum, without le or quantile), _bucket becomes count (name.bucket, with le) and quantiles become gauge (name.quantile, with quantile). 1 per unique tag combination (B2).".to_string(),
        source: PROMETHEUS_MAPPING_SOURCE,
    }
}

fn rule_d12() -> Rule {
    Rule {
        code: "D12",
        text: "OpenMetrics counter: a _total sample of a counter family counts as count on name.count, 1 per label combination (B3).".to_string(),
        source: OPENMETRICS_SOURCE,
    }
}

fn rule_d11() -> Rule {
    Rule {
        code: "D11",
        text: "Prometheus without a type: a metric with TYPE untyped or without a TYPE line counts as 0 and is shown as skipped (B3). A _count, _sum or _bucket name whose base is a counter or gauge is a separate metric without a type (D11).".to_string(),
        source: OPENMETRICS_SOURCE,
    }
}

fn rule_d5(factor: u64) -> Rule {
    Rule {
        code: "D5",
        text: with_factor(D5_TEXT, factor),
        source: CUSTOM_METRICS_SOURCE,
    }
}

fn factor_of(metric: &MetricCount) -> u64 {
    // Zero combinations means factor 0.
    metric.count.checked_div(metric.combinations).unwrap_or(0)
}

fn scale(metric: &str, series: u64, factor: u64) -> Result<u64, CountError> {
    series
        .checked_mul(factor)
        .ok_or_else(|| CountError::Product {
            metric: metric.to_string(),
            factor,
            combinations: series,
        })
}

#[cfg(test)]
fn reference_bullet<'a>(reference: &'a str, code: &str) -> &'a str {
    let prefix = format!("- {code} ");
    reference
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("docs/reference.md has no {code} bullet"))
}

#[cfg(test)]
fn text_without_factor(text: &str) -> &str {
    text.split_once(" Configured factor:")
        .map(|(text, _)| text)
        .unwrap_or_else(|| panic!("factor missing: {text}"))
}

#[cfg(test)]
mod tests {
    use super::{
        allotment, count_and_gauge, count_by_metric, count_histogram_distribution,
        reference_bullet, text_without_factor, unique_hosts, CountError, Plan,
        CUSTOM_METRICS_SOURCE, D4_TEXT, D5_TEXT, DEFAULT_HISTOGRAM_AGGREGATES,
        DEFAULT_HISTOGRAM_PERCENTILES,
    };
    use crate::parse::{parse_dogstatsd, parse_prometheus, Kind};

    fn total_of(input: &str) -> u64 {
        let samples = parse_dogstatsd(input).expect("synthetic sample");
        count_and_gauge(&samples).expect("count")
    }

    /// B1: four unique combinations of `request.Latency` (`host`, `endpoint`, `status`).
    /// The fifth line repeats the first combination and does not count again.
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

    fn count_hd(kind: &str, aggregates: u64, percentiles_n: u64, percentiles: bool) -> u64 {
        let samples = parse_dogstatsd(&b1_request_latency(kind)).expect("synthetic sample");
        count_histogram_distribution(&samples, aggregates, percentiles_n, percentiles)
            .expect("count")
    }

    #[test]
    fn request_latency_count_and_gauge_is_4() {
        for kind in ["c", "g"] {
            assert_eq!(total_of(&b1_request_latency(kind)), 4, "kind {kind}");
        }
    }

    #[test]
    fn b1_histogram_is_20() {
        assert_eq!(
            count_hd(
                "h",
                DEFAULT_HISTOGRAM_AGGREGATES,
                DEFAULT_HISTOGRAM_PERCENTILES,
                false
            ),
            20
        );
    }

    #[test]
    fn b1_distribution_is_20() {
        assert_eq!(
            count_hd(
                "d",
                DEFAULT_HISTOGRAM_AGGREGATES,
                DEFAULT_HISTOGRAM_PERCENTILES,
                false
            ),
            20
        );
    }

    #[test]
    fn b1_distribution_with_percentiles_is_40() {
        assert_eq!(
            count_hd(
                "d",
                DEFAULT_HISTOGRAM_AGGREGATES,
                DEFAULT_HISTOGRAM_PERCENTILES,
                true
            ),
            40
        );
    }

    #[test]
    fn b1_histogram_aggregates_6_percentiles_1_is_28() {
        assert_eq!(count_hd("h", 6, 1, false), 28);
    }

    #[test]
    fn temperature_2_then_3_with_city_then_3_with_state() {
        let base_sample = "\
temperature:60|g|#country:USA,region:Northeast
temperature:80|g|#country:USA,region:Southeast
";
        let with_city = "\
temperature:60|g|#country:USA,region:Northeast,city:NYC
temperature:82|g|#country:USA,region:Southeast,city:Orlando
temperature:84|g|#country:USA,region:Southeast,city:Miami
";
        let with_state = "\
temperature:60|g|#country:USA,region:Northeast,city:NYC,state:NY
temperature:82|g|#country:USA,region:Southeast,city:Orlando,state:Florida
temperature:84|g|#country:USA,region:Southeast,city:Miami,state:Florida
";
        assert_eq!(total_of(base_sample), 2);
        assert_eq!(total_of(with_city), 3);
        assert_eq!(total_of(with_state), 3);
    }

    #[test]
    fn reversed_tag_order_is_1() {
        let input = "\
temperature:80|g|#country:USA,state:Florida,city:Miami
temperature:81|g|#state:Florida,city:Miami,country:USA
";
        assert_eq!(total_of(input), 1);
    }

    #[test]
    fn report_per_metric_name_type_and_rule() {
        let input = "\
page.views:1|c|#host:a,env:prod
page.views:1|c|#env:prod,host:a
page.views:1|c|#host:b,env:prod
temp:1|g|#city:x
latency:1|h|#host:a
latency:1|h|#host:b
bytes:1|d|#host:a
";
        let samples = parse_dogstatsd(input).expect("synthetic sample");
        let report = count_by_metric(&samples, 4, 1, false).expect("count");
        let lines: Vec<_> = report
            .metrics
            .iter()
            .map(|metric| {
                (
                    metric.name.as_str(),
                    metric.kind,
                    metric.combinations,
                    metric.count,
                    metric.rule.code,
                )
            })
            .collect();
        assert_eq!(
            lines,
            vec![
                ("bytes", Kind::Distribution, 1, 5, "D5"),
                ("latency", Kind::Histogram, 2, 10, "D4"),
                ("page.views", Kind::Count, 2, 2, "D3"),
                ("temp", Kind::Gauge, 1, 1, "D3"),
            ]
        );
        let sum: u64 = report.metrics.iter().map(|metric| metric.count).sum();
        assert_eq!(report.total, sum);
        assert!(report
            .metrics
            .iter()
            .all(|metric| metric.rule.source == CUSTOM_METRICS_SOURCE));
    }

    #[test]
    fn report_histogram_factor_from_flags() {
        let samples = parse_dogstatsd(&b1_request_latency("h")).expect("synthetic sample");
        let report = count_by_metric(&samples, 6, 1, false).expect("count");
        assert_eq!(report.metrics.len(), 1);
        let metric = &report.metrics[0];
        assert_eq!(metric.combinations, 4);
        assert_eq!(metric.count, 28);
        assert_eq!(metric.rule.code, "D4");
        assert!(
            metric.rule.text.ends_with("Configured factor: 7."),
            "{}",
            metric.rule.text
        );
        assert_eq!(report.total, 28);
    }

    /// D4 and D5 rule text matches the reference. The factor is appended in the output.
    #[test]
    fn d4_and_d5_text_matches_the_reference() {
        let reference = include_str!("../docs/reference.md");
        let d4 = reference_bullet(reference, "D4");
        let d5 = reference_bullet(reference, "D5");
        assert_eq!(d4, D4_TEXT);
        assert_eq!(d5, D5_TEXT);

        let histogram = parse_dogstatsd(&b1_request_latency("h")).expect("synthetic sample");
        let default = count_by_metric(
            &histogram,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        assert_eq!(text_without_factor(&default.metrics[0].rule.text), d4);
        assert!(default.metrics[0]
            .rule
            .text
            .ends_with("Configured factor: 5."));

        let distribution = parse_dogstatsd("bytes:1|d|#host:a\n").expect("synthetic sample");
        let without = count_by_metric(&distribution, 4, 1, false).expect("count");
        let with = count_by_metric(&distribution, 4, 1, true).expect("count");
        assert_eq!(text_without_factor(&without.metrics[0].rule.text), d5);
        assert_eq!(text_without_factor(&with.metrics[0].rule.text), d5);
        assert!(without.metrics[0]
            .rule
            .text
            .ends_with("Configured factor: 5."));
        assert!(with.metrics[0]
            .rule
            .text
            .ends_with("Configured factor: 10."));
    }

    #[test]
    fn report_distribution_with_percentiles_counts_10() {
        let samples =
            parse_dogstatsd("bytes:1|d|#host:a\nbytes:1|d|#host:b\n").expect("synthetic sample");
        let report = count_by_metric(&samples, 4, 1, true).expect("count");
        assert_eq!(report.metrics[0].combinations, 2);
        assert_eq!(report.metrics[0].count, 20);
        assert_eq!(report.metrics[0].rule.code, "D5");
        assert!(report.metrics[0].rule.text.contains("p99"));
    }

    #[test]
    fn ms_is_histogram_and_set_is_gauge() {
        let samples = parse_dogstatsd("rpc.time:10|ms|#host:a\nusers:alice|s|#host:a\n")
            .expect("synthetic sample");
        let report = count_by_metric(
            &samples,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        assert_eq!(report.metrics.len(), 2);
        assert_eq!(report.metrics[0].name, "rpc.time");
        assert_eq!(report.metrics[0].kind, Kind::Histogram);
        assert_eq!(report.metrics[0].count, 5);
        assert_eq!(report.metrics[0].rule.code, "D4");
        assert_eq!(report.metrics[1].name, "users");
        assert_eq!(report.metrics[1].kind, Kind::Gauge);
        assert_eq!(report.metrics[1].count, 1);
        assert_eq!(report.metrics[1].rule.code, "D3");
    }

    #[test]
    fn mixed_count_and_gauge_in_one_sample() {
        // Interleaved: count, gauge, count, gauge, the same count again, gauge.
        let input = "\
page.views:1|c|#host:a,env:prod
queue.depth:3|g|#host:a
page.views:1|c|#host:b,env:prod
queue.depth:4|g|#host:b
page.views:2|c|#env:prod,host:a
temperature:1|g|#city:x
";
        let samples = parse_dogstatsd(input).expect("synthetic sample");
        let report = count_by_metric(
            &samples,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        let per: Vec<_> = report
            .metrics
            .iter()
            .map(|metric| (metric.name.as_str(), metric.kind, metric.count))
            .collect();
        assert_eq!(
            per,
            vec![
                ("page.views", Kind::Count, 2),
                ("queue.depth", Kind::Gauge, 2),
                ("temperature", Kind::Gauge, 1),
            ]
        );
        assert_eq!(report.total, 5);
        assert_eq!(total_of(input), 5);
    }

    #[test]
    fn pro_with_3_hosts_is_300() {
        let outcome = allotment(Plan::Pro, 3, 4, None).expect("allotment");
        assert_eq!(outcome.plan, Plan::Pro);
        assert_eq!(outcome.hosts, 3);
        assert_eq!(outcome.allotment, 300);
        assert_eq!(outcome.indexed.usage, 4);
        assert_eq!(outcome.indexed.surplus, 0);
        assert!(outcome.ingested.is_none());
    }

    #[test]
    fn enterprise_with_2_hosts_is_400() {
        let outcome = allotment(Plan::Enterprise, 2, 4, None).expect("allotment");
        assert_eq!(outcome.allotment, 400);
        assert_eq!(outcome.indexed.usage, 4);
        assert_eq!(outcome.indexed.surplus, 0);
    }

    #[test]
    fn indexed_and_ingested_are_separate_against_the_allotment() {
        let outcome = allotment(Plan::Pro, 1, 150, Some(80)).expect("allotment");
        assert_eq!(outcome.allotment, 100);
        assert_eq!(outcome.indexed.usage, 150);
        assert_eq!(outcome.indexed.surplus, 50);
        let ingested = outcome.ingested.expect("ingested");
        assert_eq!(ingested.usage, 80);
        assert_eq!(ingested.surplus, 0);
    }

    #[test]
    fn hosts_times_per_host_too_large_is_an_error() {
        let err = allotment(Plan::Pro, u64::MAX, 0, None).expect_err("too large");
        assert_eq!(err, CountError::Allotment);
        let text = err.to_string();
        assert!(text.contains("allotment does not fit"), "{text}");
    }

    #[test]
    fn histogram_percentile_too_large_is_an_error() {
        let samples = parse_dogstatsd("latency:1|h|#host:a\n").expect("synthetic sample");
        let err = count_by_metric(&samples, DEFAULT_HISTOGRAM_AGGREGATES, u64::MAX, false)
            .expect_err("too large");
        let text = err.to_string();
        assert!(text.contains("does not fit"), "{text}");
        assert!(text.contains("the sum"), "{text}");
        assert!(text.contains("--histogram-aggregates"), "{text}");
        assert!(text.contains("--histogram-percentiles"), "{text}");
        assert!(
            text.contains(&DEFAULT_HISTOGRAM_AGGREGATES.to_string()),
            "{text}"
        );
        assert!(text.contains(&u64::MAX.to_string()), "{text}");
        assert!(!text.contains("host:a"), "{text}");
    }

    #[test]
    fn without_a_histogram_the_factor_is_not_used() {
        let cases = [
            (
                "page.views:1|c|#host:a\npage.views:1|c|#host:b\n",
                false,
                2u64,
            ),
            ("temp:1|g|#city:x\ntemp:1|g|#city:y\n", false, 2),
            ("bytes:1|d|#host:a\nbytes:1|d|#host:b\n", false, 10),
            ("bytes:1|d|#host:a\nbytes:1|d|#host:b\n", true, 20),
        ];
        for (input, percentiles, expected) in cases {
            let samples = parse_dogstatsd(input).expect("synthetic sample");
            let report = count_by_metric(&samples, u64::MAX, u64::MAX, percentiles)
                .expect("count without a histogram");
            assert_eq!(report.total, expected, "{input}");
        }

        let prom = "\
# TYPE lat histogram
lat_bucket{host=\"a\",le=\"1\"} 1
lat_count{host=\"a\"} 1
lat_sum{host=\"a\"} 1
";
        let samples = parse_prometheus(prom).expect("synthetic sample");
        let report = count_by_metric(&samples, u64::MAX, u64::MAX, false).expect("d10");
        assert_eq!(report.total, 3);
        assert!(report
            .metrics
            .iter()
            .all(|metric| metric.rule.code == "D10"));
    }

    #[test]
    fn total_overflow_names_metric_factor_and_combinations() {
        // Each metric: 2 combinations times 9223372036854775807 fits. The sum of those two does not.
        let input = "\
queue.wait:1|h|#host:h1
queue.wait:1|h|#host:h2
request.latency:1|h|#host:h1
request.latency:1|h|#host:h2
";
        let samples = parse_dogstatsd(input).expect("synthetic sample");
        let factor = 9_223_372_036_854_775_807u64;
        let err = count_by_metric(&samples, factor, 0, false).expect_err("total");
        let text = err.to_string();
        assert!(text.contains("request.latency"), "{text}");
        assert!(!text.contains("queue.wait"), "{text}");
        assert!(text.contains(&factor.to_string()), "{text}");
        assert!(text.contains("2 combinations"), "{text}");
        assert!(text.contains("total"), "{text}");
        assert!(!text.contains("the sum"), "{text}");
        assert!(!text.contains("h1"), "{text}");
        assert!(!text.contains("h2"), "{text}");
    }

    #[test]
    fn unique_host_values_from_the_sample() {
        let input = "\
page.views:1|c|#host:a,env:prod
page.views:1|c|#hostname:box,host:a
page.views:1|c|#host:b
page.views:1|c|#host
page.views:1|c|#env:prod
";
        let samples = parse_dogstatsd(input).expect("synthetic sample");
        assert_eq!(unique_hosts(&samples), 2);
        assert_eq!(
            allotment(Plan::Pro, unique_hosts(&samples), 4, None)
                .expect("allotment")
                .allotment,
            200
        );
    }

    #[test]
    fn surplus_is_zero_when_underutilized() {
        assert_eq!(
            allotment(Plan::Pro, 3, 0, None)
                .expect("allotment")
                .indexed
                .surplus,
            0
        );
        assert_eq!(
            allotment(Plan::Pro, 3, 299, None)
                .expect("allotment")
                .indexed
                .surplus,
            0
        );
        assert_eq!(
            allotment(Plan::Enterprise, 2, 1, None)
                .expect("allotment")
                .indexed
                .surplus,
            0
        );
        assert_eq!(
            allotment(Plan::Pro, 3, 300, None)
                .expect("allotment")
                .indexed
                .surplus,
            0
        );
    }

    #[test]
    fn surplus_is_usage_above_the_allotment() {
        assert_eq!(
            allotment(Plan::Pro, 3, 301, None)
                .expect("allotment")
                .indexed
                .surplus,
            1
        );
        assert_eq!(
            allotment(Plan::Enterprise, 2, 450, None)
                .expect("allotment")
                .indexed
                .surplus,
            50
        );
    }
}

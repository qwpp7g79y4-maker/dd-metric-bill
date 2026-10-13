use std::collections::{BTreeMap, BTreeSet};

use crate::count::{count_by_metric, CountError};
use crate::parse::{Kind, Sample};
use crate::tags::tag_breakdown;

/// Difference between two samples.
///
/// `delta` is the new total minus the old one. Tag values are not included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReport {
    pub old_total: u64,
    pub new_total: u64,
    pub delta: i128,
    pub metrics: Vec<MetricDelta>,
}

/// Difference for one metric name and kind.
///
/// `old_count` is `None` when the metric is only in the new sample.
/// `new_count` is `None` when it is only in the old sample.
/// `delta` is the billable count (new minus old; absent counts as 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricDelta {
    pub name: String,
    pub kind: Kind,
    pub old_count: Option<u64>,
    pub new_count: Option<u64>,
    pub delta: i128,
    pub rule: &'static str,
    pub tags: Vec<TagDelta>,
}

/// Difference for one tag key within one metric.
///
/// `delta` is the difference in added series (D1, D2): the count with the
/// key minus the count without it. The histogram and distribution factor is
/// not included, same as in the `count` breakdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagDelta {
    pub key: String,
    pub old_values: u64,
    pub new_values: u64,
    pub old_added: u64,
    pub new_added: u64,
    pub delta: i128,
}

struct TagSide {
    values: u64,
    added: u64,
}

struct Side {
    kind: Kind,
    count: u64,
    rule: &'static str,
    tags: BTreeMap<String, TagSide>,
}

struct Sides {
    total: u64,
    metrics: BTreeMap<(String, u8), Side>,
}

/// Compares two samples with the same counting rules as `count`.
///
/// New and removed metrics, the difference per metric and per tag key,
/// and the total difference. `top` keeps, per metric, the tag keys with the
/// largest absolute difference in added series; `None` keeps all of them.
///
/// # Errors
///
/// [`CountError`] when the count does not fit in `u64`.
pub fn diff_samples(
    old: &[Sample],
    new: &[Sample],
    histogram_aggregates: u64,
    histogram_percentiles: u64,
    percentiles: bool,
    top: Option<u64>,
) -> Result<DiffReport, CountError> {
    let old = sides(
        old,
        histogram_aggregates,
        histogram_percentiles,
        percentiles,
    )?;
    let new = sides(
        new,
        histogram_aggregates,
        histogram_percentiles,
        percentiles,
    )?;
    let keys: BTreeSet<(String, u8)> = old
        .metrics
        .keys()
        .chain(new.metrics.keys())
        .cloned()
        .collect();
    let metrics = keys
        .iter()
        .map(|key| {
            let left = old.metrics.get(key);
            let right = new.metrics.get(key);
            let side = left
                .or(right)
                .expect("key comes from the old or the new sample");
            let old_count = left.map(|item| item.count);
            let new_count = right.map(|item| item.count);
            MetricDelta {
                name: key.0.clone(),
                kind: side.kind,
                old_count,
                new_count,
                delta: delta(old_count.unwrap_or(0), new_count.unwrap_or(0)),
                rule: side.rule,
                tags: tag_deltas(
                    left.map(|item| &item.tags),
                    right.map(|item| &item.tags),
                    top,
                ),
            }
        })
        .collect();
    Ok(DiffReport {
        old_total: old.total,
        new_total: new.total,
        delta: delta(old.total, new.total),
        metrics,
    })
}

fn sides(
    samples: &[Sample],
    histogram_aggregates: u64,
    histogram_percentiles: u64,
    percentiles: bool,
) -> Result<Sides, CountError> {
    let report = count_by_metric(
        samples,
        histogram_aggregates,
        histogram_percentiles,
        percentiles,
    )?;
    let tags = tag_breakdown(samples, None);
    let mut map = BTreeMap::new();
    for metric in report.metrics {
        let tag_map = tags
            .iter()
            .find(|item| item.name == metric.name && item.kind == metric.kind)
            .map(|item| {
                item.keys
                    .iter()
                    .map(|tag| {
                        (
                            tag.key.clone(),
                            TagSide {
                                values: tag.unique_values,
                                added: tag.added,
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        map.insert(
            (metric.name, kind_ord(metric.kind)),
            Side {
                kind: metric.kind,
                count: metric.count,
                rule: metric.rule.code,
                tags: tag_map,
            },
        );
    }
    Ok(Sides {
        total: report.total,
        metrics: map,
    })
}

fn tag_deltas(
    old: Option<&BTreeMap<String, TagSide>>,
    new: Option<&BTreeMap<String, TagSide>>,
    top: Option<u64>,
) -> Vec<TagDelta> {
    let empty = BTreeMap::new();
    let old = old.unwrap_or(&empty);
    let new = new.unwrap_or(&empty);
    let names: BTreeSet<&str> = old.keys().chain(new.keys()).map(String::as_str).collect();
    let mut tags: Vec<TagDelta> = names
        .into_iter()
        .map(|name| {
            let old = old.get(name);
            let new = new.get(name);
            let old_added = old.map_or(0, |tag| tag.added);
            let new_added = new.map_or(0, |tag| tag.added);
            TagDelta {
                key: name.to_string(),
                old_values: old.map_or(0, |tag| tag.values),
                new_values: new.map_or(0, |tag| tag.values),
                old_added,
                new_added,
                delta: delta(old_added, new_added),
            }
        })
        .collect();
    tags.sort_by(|a, b| {
        b.delta
            .unsigned_abs()
            .cmp(&a.delta.unsigned_abs())
            .then(a.key.cmp(&b.key))
    });
    if let Some(max) = top {
        let max = usize::try_from(max).expect("top fits in usize");
        tags.truncate(max);
    }
    tags
}

/// New count minus old count. Every pair of `u64` counts fits in `i128`.
fn delta(old: u64, new: u64) -> i128 {
    i128::from(new) - i128::from(old)
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

#[cfg(test)]
mod tests {
    use super::diff_samples;
    use crate::parse::{parse_dogstatsd, Kind, Sample};

    fn fixture(name: &str) -> Vec<Sample> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("reading fixture {}: {err}", path.display()));
        parse_dogstatsd(&text).expect("synthetic fixture")
    }

    #[test]
    fn tag_added_and_metric_removed() {
        let report = diff_samples(
            &fixture("diff-old.dogstatsd"),
            &fixture("diff-new.dogstatsd"),
            4,
            1,
            false,
            None,
        )
        .expect("count");
        assert_eq!(report.old_total, 3);
        assert_eq!(report.new_total, 4);
        assert_eq!(report.delta, 1);

        let checkout = &report.metrics[0];
        assert_eq!(checkout.name, "checkout");
        assert_eq!(checkout.kind, Kind::Count);
        assert_eq!(checkout.old_count, None);
        assert_eq!(checkout.new_count, Some(1));
        assert_eq!(checkout.delta, 1);
        assert_eq!(checkout.rule, "D3");
        assert_eq!(checkout.tags.len(), 1);
        assert_eq!(checkout.tags[0].key, "env");
        assert_eq!(checkout.tags[0].new_values, 1);
        assert_eq!(checkout.tags[0].delta, 0);

        let page = &report.metrics[1];
        assert_eq!(page.name, "page.views");
        assert_eq!(page.old_count, Some(2));
        assert_eq!(page.new_count, Some(3));
        assert_eq!(page.delta, 1);
        assert_eq!(page.tags[0].key, "region");
        assert_eq!(
            (
                page.tags[0].old_values,
                page.tags[0].new_values,
                page.tags[0].old_added,
                page.tags[0].new_added,
                page.tags[0].delta,
            ),
            (0, 2, 0, 1, 1)
        );
        let env = page.tags.iter().find(|tag| tag.key == "env").expect("env");
        assert_eq!(
            (
                env.old_values,
                env.new_values,
                env.old_added,
                env.new_added,
                env.delta
            ),
            (1, 1, 0, 0, 0)
        );
        let host = page
            .tags
            .iter()
            .find(|tag| tag.key == "host")
            .expect("host");
        assert_eq!(
            (
                host.old_values,
                host.new_values,
                host.old_added,
                host.new_added,
                host.delta,
            ),
            (2, 2, 1, 1, 0)
        );

        let queue = &report.metrics[2];
        assert_eq!(queue.name, "queue.depth");
        assert_eq!(queue.kind, Kind::Gauge);
        assert_eq!(queue.old_count, Some(1));
        assert_eq!(queue.new_count, None);
        assert_eq!(queue.delta, -1);
        assert_eq!(queue.tags[0].key, "host");
        assert_eq!(queue.tags[0].old_values, 1);
        assert_eq!(queue.tags[0].new_values, 0);
        assert_eq!(queue.tags[0].delta, 0);
    }

    #[test]
    fn top_limits_only_the_tag_keys() {
        let report = diff_samples(
            &fixture("diff-old.dogstatsd"),
            &fixture("diff-new.dogstatsd"),
            4,
            1,
            false,
            Some(1),
        )
        .expect("count");
        assert_eq!(report.metrics.len(), 3);
        let page = report
            .metrics
            .iter()
            .find(|metric| metric.name == "page.views")
            .expect("page.views");
        assert_eq!(page.tags.len(), 1);
        assert_eq!(page.tags[0].key, "region");
    }

    #[test]
    fn histogram_uses_the_same_factor() {
        let old = parse_dogstatsd("latency:1|h|#host:a\n").expect("synthetic sample");
        let new = parse_dogstatsd("latency:1|h|#host:a\nlatency:1|h|#host:b\n")
            .expect("synthetic sample");
        let report = diff_samples(&old, &new, 4, 1, false, None).expect("count");
        assert_eq!(report.old_total, 5);
        assert_eq!(report.new_total, 10);
        assert_eq!(report.delta, 5);
        assert_eq!(report.metrics[0].rule, "D4");
        assert_eq!(report.metrics[0].delta, 5);
        assert_eq!(report.metrics[0].tags[0].key, "host");
        assert_eq!(report.metrics[0].tags[0].delta, 1);
    }

    #[test]
    fn distribution_percentiles_count_in_the_difference() {
        let old = parse_dogstatsd("bytes:1|d|#host:a\n").expect("synthetic sample");
        let new =
            parse_dogstatsd("bytes:1|d|#host:a\nbytes:1|d|#host:b\n").expect("synthetic sample");
        let without = diff_samples(&old, &new, 4, 1, false, None).expect("count");
        let with = diff_samples(&old, &new, 4, 1, true, None).expect("count");
        assert_eq!(
            (without.old_total, without.new_total, without.delta),
            (5, 10, 5)
        );
        assert_eq!((with.old_total, with.new_total, with.delta), (10, 20, 10));
        assert_eq!(with.metrics[0].rule, "D5");
    }

    #[test]
    fn empty_samples_have_zero_difference() {
        let report = diff_samples(&[], &[], 4, 1, false, None).expect("count");
        assert_eq!(report.old_total, 0);
        assert_eq!(report.new_total, 0);
        assert_eq!(report.delta, 0);
        assert!(report.metrics.is_empty());
    }
}

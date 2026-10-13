use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;

use serde_json::{json, Map, Value};

use crate::allow::{AllowReport, MetricAllow};
use crate::cost::CostReport;
use crate::count::{Allotment, CountReport, MetricCount, Plan, CUSTOM_METRICS_SOURCE};
use crate::diff::{DiffReport, MetricDelta, TagDelta};
use crate::parse::Kind;
use crate::tags::{MetricTags, MetricUnbounded, TagKey, UnboundedReason, UnboundedTag};

const TOOL: &str = "dd-metric-bill";

/// Makes control characters visible, so they do not drive the terminal.
/// JSON does not use this function: the raw string stays there.
fn visible(text: &str) -> String {
    crate::count::visible_text(text)
}

/// Fixed line in every text output (D8). A sample is one snapshot.
pub const ESTIMATE_LINE: &str =
    "estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)";

/// Text for `count`.
///
/// Each metric shows name, type, unique combinations, billable count, and
/// rule code. With `explain`, the rule text and the source URL follow. Then,
/// per metric, the tag keys from `tags`: unique values and added series.
/// Unbounded keys from `unbounded` follow with their reason. Tag values
/// are not included. With an allowlist (D6), `ingested` and `indexed` follow
/// per metric; without an allowlist those lines are absent. With an allotment,
/// the allotment follows. Without an allowlist, usage is the total (indexed).
/// With an allowlist, indexed and ingested are each set against the allotment
/// separately. With a price file, currency, source, date, and the cost above
/// the allotment follow. The estimate line is always at the bottom.
#[must_use]
pub fn render_count(
    report: &CountReport,
    explain: bool,
    allotment: Option<&Allotment>,
    tags: &[MetricTags],
    unbounded: &[MetricUnbounded],
    allow: Option<&AllowReport>,
    cost: Option<&CostReport>,
) -> String {
    let index = Index::build(tags, unbounded, allow);
    let mut out = String::new();
    writeln!(out, "total: {}", report.total).expect("writing to a String does not fail");
    if let Some(allotment) = allotment {
        write_allotment(&mut out, allotment);
    }
    if let Some(cost) = cost {
        write_cost(&mut out, cost);
    }
    if !report.metrics.is_empty() {
        out.push('\n');
        for metric in &report.metrics {
            write_metric(
                &mut out,
                metric,
                explain,
                index.keys(&metric.name, metric.kind),
                index.unbounded(&metric.name, metric.kind),
                index.allow(&metric.name, metric.kind),
            );
        }
        out.push('\n');
    }
    out.push_str(ESTIMATE_LINE);
    out.push('\n');
    out
}

/// Text for `diff`.
///
/// First the old total, the new total, and the total difference. Then the
/// metrics that are only in the new sample, and the metrics that were only
/// in the old sample. Per metric, the difference in billable count follows
/// and, per tag key, the difference in unique values and added series. Tag
/// values are not included. The estimate line is always at the bottom.
#[must_use]
pub fn render_diff(report: &DiffReport) -> String {
    let mut out = String::new();
    writeln!(out, "old total: {}", report.old_total).expect("writing to a String does not fail");
    writeln!(out, "new total: {}", report.new_total).expect("writing to a String does not fail");
    writeln!(out, "delta: {}", report.delta).expect("writing to a String does not fail");

    let new: Vec<&MetricDelta> = report
        .metrics
        .iter()
        .filter(|metric| metric.old_count.is_none())
        .collect();
    let dropped: Vec<&MetricDelta> = report
        .metrics
        .iter()
        .filter(|metric| metric.new_count.is_none())
        .collect();
    if !new.is_empty() {
        out.push('\n');
        writeln!(out, "new:").expect("writing to a String does not fail");
        for metric in new {
            writeln!(
                out,
                "{name} type: {kind} count: {count}",
                name = visible(&metric.name),
                kind = kind_label(metric.kind),
                count = metric.new_count.unwrap_or(0),
            )
            .expect("writing to a String does not fail");
        }
    }
    if !dropped.is_empty() {
        out.push('\n');
        writeln!(out, "removed:").expect("writing to a String does not fail");
        for metric in dropped {
            writeln!(
                out,
                "{name} type: {kind} count: {count}",
                name = visible(&metric.name),
                kind = kind_label(metric.kind),
                count = metric.old_count.unwrap_or(0),
            )
            .expect("writing to a String does not fail");
        }
    }
    if !report.metrics.is_empty() {
        out.push('\n');
        for metric in &report.metrics {
            write_delta(&mut out, metric);
        }
        out.push('\n');
    }
    out.push_str(ESTIMATE_LINE);
    out.push('\n');
    out
}

/// JSON for `count`, `schema_version` 1.
///
/// Keys: `schema_version`, `tool`, `estimate`, `source`, `inputs`, `total`,
/// `metrics`, `tags`, `unbounded`, and `allotment` (`null` without `--plan`).
/// `ingested` and `indexed` only with an allowlist. `costs` only with a
/// price file. Tag values are not included.
#[must_use]
pub fn render_count_json(
    inputs: &[impl AsRef<Path>],
    report: &CountReport,
    allotment: Option<&Allotment>,
    tags: &[MetricTags],
    unbounded: &[MetricUnbounded],
    allow: Option<&AllowReport>,
    cost: Option<&CostReport>,
) -> String {
    let index = Index::build(tags, unbounded, allow);
    let mut doc = base_document(
        json!(inputs
            .iter()
            .map(|path| input_path(path.as_ref()))
            .collect::<Vec<_>>()),
        json!(report.total),
        metrics_json(report, &index),
        tags_json(tags),
        unbounded_json(unbounded),
        allotment.map(allotment_json).unwrap_or(Value::Null),
    );
    if let Some(allow) = allow {
        if let Some(ingested) = allow.ingested {
            doc.insert("ingested".to_string(), json!(ingested));
            doc.insert("indexed".to_string(), json!(allow.indexed));
        }
    }
    if let Some(cost) = cost {
        doc.insert("costs".to_string(), cost_json(cost));
    }
    to_json(&Value::Object(doc))
}

/// JSON for `diff`, `schema_version` 1.
///
/// The same keys as `count`. `total` is old, new, and difference.
/// `inputs` is old and new. `allotment` is `null` and `unbounded` is empty:
/// `diff` does not compute those. `ingested`, `indexed`, and `costs` are
/// absent. Tag values are not included.
#[must_use]
pub fn render_diff_json(old: &Path, new: &Path, report: &DiffReport) -> String {
    let doc = base_document(
        json!({
            "old": input_path(old),
            "new": input_path(new),
        }),
        json!({
            "old": report.old_total,
            "new": report.new_total,
            "delta": report.delta,
        }),
        Value::Array(report.metrics.iter().map(delta_json).collect()),
        Value::Array(
            report
                .metrics
                .iter()
                .filter(|metric| !metric.tags.is_empty())
                .map(delta_tags_json)
                .collect(),
        ),
        json!([]),
        Value::Null,
    );
    to_json(&Value::Object(doc))
}

fn base_document(
    inputs: Value,
    total: Value,
    metrics: Value,
    tags: Value,
    unbounded: Value,
    allotment: Value,
) -> Map<String, Value> {
    let mut doc = Map::new();
    doc.insert("schema_version".to_string(), json!(1));
    doc.insert("tool".to_string(), json!(TOOL));
    doc.insert("estimate".to_string(), json!(ESTIMATE_LINE));
    doc.insert("source".to_string(), json!(CUSTOM_METRICS_SOURCE));
    doc.insert("inputs".to_string(), inputs);
    doc.insert("total".to_string(), total);
    doc.insert("metrics".to_string(), metrics);
    doc.insert("tags".to_string(), tags);
    doc.insert("unbounded".to_string(), unbounded);
    doc.insert("allotment".to_string(), allotment);
    doc
}

fn to_json(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("json fits in a string");
    text.push('\n');
    text
}

fn input_path(path: &Path) -> String {
    path.display().to_string()
}

fn metrics_json(report: &CountReport, index: &Index<'_>) -> Value {
    Value::Array(
        report
            .metrics
            .iter()
            .map(|metric| metric_json(metric, index.allow(&metric.name, metric.kind)))
            .collect(),
    )
}

fn metric_json(metric: &MetricCount, allow: Option<&MetricAllow>) -> Value {
    let mut obj = Map::new();
    obj.insert("name".to_string(), json!(metric.name));
    obj.insert("type".to_string(), json!(kind_label(metric.kind)));
    obj.insert("combinations".to_string(), json!(metric.combinations));
    obj.insert("count".to_string(), json!(metric.count));
    obj.insert("rule".to_string(), json!(metric.rule.code));
    obj.insert("rule_text".to_string(), json!(metric.rule.text));
    if let Some((ingested, indexed)) =
        allow.and_then(|item| item.ingested.map(|ingested| (ingested, item.indexed)))
    {
        obj.insert("ingested".to_string(), json!(ingested));
        obj.insert("indexed".to_string(), json!(indexed));
    }
    Value::Object(obj)
}

fn tags_json(tags: &[MetricTags]) -> Value {
    Value::Array(tags.iter().map(metric_tags_json).collect())
}

fn metric_tags_json(metric: &MetricTags) -> Value {
    json!({
        "name": metric.name,
        "type": kind_label(metric.kind),
        "keys": metric
            .keys
            .iter()
            .map(|tag| json!({
                "key": tag.key,
                "values": tag.unique_values,
                "added": tag.added,
            }))
            .collect::<Vec<_>>(),
    })
}

fn unbounded_json(unbounded: &[MetricUnbounded]) -> Value {
    Value::Array(unbounded.iter().map(metric_unbounded_json).collect())
}

fn metric_unbounded_json(metric: &MetricUnbounded) -> Value {
    json!({
        "name": metric.name,
        "type": kind_label(metric.kind),
        "keys": metric
            .keys
            .iter()
            .map(|tag| json!({
                "key": tag.key,
                "reasons": tag.reasons.iter().copied().map(reason_label).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
    })
}

fn allotment_json(allotment: &Allotment) -> Value {
    let mut obj = Map::new();
    obj.insert("plan".to_string(), json!(plan_label(allotment.plan)));
    obj.insert("hosts".to_string(), json!(allotment.hosts));
    obj.insert("allotment".to_string(), json!(allotment.allotment));
    if let Some(ingested) = allotment.ingested {
        obj.insert("indexed".to_string(), json!(allotment.indexed.usage));
        obj.insert(
            "indexed_surplus".to_string(),
            json!(allotment.indexed.surplus),
        );
        obj.insert("ingested".to_string(), json!(ingested.usage));
        obj.insert("ingested_surplus".to_string(), json!(ingested.surplus));
    } else {
        obj.insert("usage".to_string(), json!(allotment.indexed.usage));
        obj.insert("basis".to_string(), json!("total (indexed)"));
        obj.insert("surplus".to_string(), json!(allotment.indexed.surplus));
    }
    Value::Object(obj)
}

fn cost_json(cost: &CostReport) -> Value {
    json!({
        "currency": cost.currency,
        "source": cost.source,
        "date": cost.date,
        "lines": cost
            .lines
            .iter()
            .map(|line| json!({
                "kind": line.kind.label(),
                "above": line.above,
                "amount": line.amount,
            }))
            .collect::<Vec<_>>(),
    })
}

fn delta_json(metric: &MetricDelta) -> Value {
    json!({
        "name": metric.name,
        "type": kind_label(metric.kind),
        "old": metric.old_count.unwrap_or(0),
        "new": metric.new_count.unwrap_or(0),
        "delta": metric.delta,
        "rule": metric.rule,
    })
}

fn delta_tags_json(metric: &MetricDelta) -> Value {
    json!({
        "name": metric.name,
        "type": kind_label(metric.kind),
        "keys": metric.tags.iter().map(tag_delta_json).collect::<Vec<_>>(),
    })
}

fn tag_delta_json(tag: &TagDelta) -> Value {
    json!({
        "key": tag.key,
        "old_values": tag.old_values,
        "new_values": tag.new_values,
        "old_added": tag.old_added,
        "new_added": tag.new_added,
        "delta": tag.delta,
    })
}

fn plan_label(plan: Plan) -> &'static str {
    match plan {
        Plan::Pro => "pro",
        Plan::Enterprise => "enterprise",
    }
}

fn reason_label(reason: UnboundedReason) -> &'static str {
    match reason {
        UnboundedReason::Threshold => "threshold",
        UnboundedReason::Uuid => "pattern uuid",
        UnboundedReason::HexId => "pattern hex-id",
        UnboundedReason::NumericId => "pattern numeric-id",
        UnboundedReason::PodName => "pattern pod-name",
    }
}

fn write_delta(out: &mut String, metric: &MetricDelta) {
    writeln!(
        out,
        "{name} type: {kind} old: {old} new: {new} delta: {delta} rule: {rule}",
        name = visible(&metric.name),
        kind = kind_label(metric.kind),
        old = metric.old_count.unwrap_or(0),
        new = metric.new_count.unwrap_or(0),
        delta = metric.delta,
        rule = metric.rule,
    )
    .expect("writing to a String does not fail");
    for tag in &metric.tags {
        writeln!(
            out,
            "  tag: {key} values: {old} to {new} added: {to_old} to {to_new} delta: {delta}",
            key = visible(&tag.key),
            old = tag.old_values,
            new = tag.new_values,
            to_old = tag.old_added,
            to_new = tag.new_added,
            delta = tag.delta,
        )
        .expect("writing to a String does not fail");
    }
}

fn write_allotment(out: &mut String, allotment: &Allotment) {
    writeln!(out, "allotment: {}", allotment.allotment).expect("writing to a String does not fail");
    if let Some(ingested) = allotment.ingested {
        writeln!(
            out,
            "indexed: {} surplus: {}",
            allotment.indexed.usage, allotment.indexed.surplus
        )
        .expect("writing to a String does not fail");
        writeln!(
            out,
            "ingested: {} surplus: {}",
            ingested.usage, ingested.surplus
        )
        .expect("writing to a String does not fail");
    } else {
        writeln!(out, "usage: {} (total, indexed)", allotment.indexed.usage)
            .expect("writing to a String does not fail");
        writeln!(out, "surplus: {}", allotment.indexed.surplus)
            .expect("writing to a String does not fail");
    }
}

fn write_cost(out: &mut String, cost: &CostReport) {
    let currency = visible(&cost.currency);
    writeln!(out, "price: {currency} per 100").expect("writing to a String does not fail");
    writeln!(out, "source: {}", visible(&cost.source)).expect("writing to a String does not fail");
    writeln!(out, "date: {}", visible(&cost.date)).expect("writing to a String does not fail");
    for line in &cost.lines {
        writeln!(
            out,
            "cost {kind}: {amount} {currency}",
            kind = line.kind.label(),
            amount = line.amount,
        )
        .expect("writing to a String does not fail");
    }
}

fn write_metric(
    out: &mut String,
    metric: &MetricCount,
    explain: bool,
    keys: &[TagKey],
    unbounded: &[UnboundedTag],
    allow: Option<&MetricAllow>,
) {
    writeln!(
        out,
        "{name} type: {kind} combinations: {combinations} count: {count} rule: {rule}",
        name = visible(&metric.name),
        kind = kind_label(metric.kind),
        combinations = metric.combinations,
        count = metric.count,
        rule = metric.rule.code,
    )
    .expect("writing to a String does not fail");
    if let Some((ingested, indexed)) =
        allow.and_then(|item| item.ingested.map(|ingested| (ingested, item.indexed)))
    {
        writeln!(out, "  ingested: {ingested} indexed: {indexed}")
            .expect("writing to a String does not fail");
    }
    if explain {
        writeln!(
            out,
            "  {code}: {text}",
            code = metric.rule.code,
            text = metric.rule.text
        )
        .expect("writing to a String does not fail");
        writeln!(out, "  source: {source}", source = metric.rule.source)
            .expect("writing to a String does not fail");
    }
    for tag in keys {
        writeln!(
            out,
            "  tag: {key} values: {values} added: {added}",
            key = visible(&tag.key),
            values = tag.unique_values,
            added = tag.added,
        )
        .expect("writing to a String does not fail");
    }
    for tag in unbounded {
        if tag.reasons.is_empty() {
            continue;
        }
        writeln!(
            out,
            "  unbounded: {key} reason: {reason}",
            key = visible(&tag.key),
            reason = reason_text(&tag.reasons),
        )
        .expect("writing to a String does not fail");
    }
}

/// First match for each metric name and kind. Output order stays the report order.
struct Index<'a> {
    tags: HashMap<(&'a str, u8), &'a [TagKey]>,
    unbounded: HashMap<(&'a str, u8), &'a [UnboundedTag]>,
    allow: HashMap<(&'a str, u8), &'a MetricAllow>,
}

const NO_KEYS: &[TagKey] = &[];
const NO_UNBOUNDED: &[UnboundedTag] = &[];

impl<'a> Index<'a> {
    fn build(
        tags: &'a [MetricTags],
        unbounded: &'a [MetricUnbounded],
        allow: Option<&'a AllowReport>,
    ) -> Self {
        let mut tags_map = HashMap::with_capacity(tags.len());
        for item in tags {
            tags_map
                .entry((item.name.as_str(), kind_ord(item.kind)))
                .or_insert(item.keys.as_slice());
        }
        let mut unbounded_map = HashMap::with_capacity(unbounded.len());
        for item in unbounded {
            unbounded_map
                .entry((item.name.as_str(), kind_ord(item.kind)))
                .or_insert(item.keys.as_slice());
        }
        let allow_metrics = allow.map(|report| report.metrics.as_slice()).unwrap_or(&[]);
        let mut allow_map = HashMap::with_capacity(allow_metrics.len());
        for item in allow_metrics {
            allow_map
                .entry((item.name.as_str(), kind_ord(item.kind)))
                .or_insert(item);
        }
        Self {
            tags: tags_map,
            unbounded: unbounded_map,
            allow: allow_map,
        }
    }

    fn keys(&self, name: &str, kind: Kind) -> &'a [TagKey] {
        self.tags
            .get(&(name, kind_ord(kind)))
            .copied()
            .unwrap_or(NO_KEYS)
    }

    fn unbounded(&self, name: &str, kind: Kind) -> &'a [UnboundedTag] {
        self.unbounded
            .get(&(name, kind_ord(kind)))
            .copied()
            .unwrap_or(NO_UNBOUNDED)
    }

    fn allow(&self, name: &str, kind: Kind) -> Option<&'a MetricAllow> {
        self.allow.get(&(name, kind_ord(kind))).copied()
    }
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

fn reason_text(reasons: &[UnboundedReason]) -> String {
    reasons
        .iter()
        .copied()
        .map(reason_label)
        .collect::<Vec<_>>()
        .join(", ")
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Count => "count",
        Kind::Gauge => "gauge",
        Kind::Histogram => "histogram",
        Kind::Distribution => "distribution",
        Kind::Untyped => "skipped",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::{render_count, render_count_json, render_diff, render_diff_json, ESTIMATE_LINE};
    use crate::allow::{AllowReport, MetricAllow};
    use crate::cost::{CostKind, CostLine, CostReport};
    use crate::count::{allotment, CountReport, MetricCount, Plan, Rule, CUSTOM_METRICS_SOURCE};
    use crate::diff::{DiffReport, MetricDelta, TagDelta};
    use crate::parse::Kind;
    use crate::tags::{MetricTags, MetricUnbounded, TagKey, UnboundedReason, UnboundedTag};
    use serde_json::Value;

    fn metric(
        name: &str,
        kind: Kind,
        combinations: u64,
        count: u64,
        code: &'static str,
        text: &str,
    ) -> MetricCount {
        MetricCount {
            name: name.to_string(),
            kind,
            combinations,
            count,
            rule: Rule {
                code,
                text: text.to_string(),
                source: CUSTOM_METRICS_SOURCE,
            },
        }
    }

    #[test]
    fn text_shows_total_per_metric_and_estimate() {
        let report = CountReport {
            total: 6,
            metrics: vec![
                metric(
                    "page.views",
                    Kind::Count,
                    2,
                    2,
                    "D3",
                    "COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).",
                ),
                metric("rpc.latency", Kind::Histogram, 1, 4, "D4", "HISTOGRAM: example."),
            ],
        };
        let text = render_count(&report, false, None, &[], &[], None, None);
        assert_eq!(
            text,
            "\
total: 6

page.views type: count combinations: 2 count: 2 rule: D3
rpc.latency type: histogram combinations: 1 count: 4 rule: D4

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
"
        );
        assert_eq!(text.matches(ESTIMATE_LINE).count(), 1);
        assert!(!text.contains("https://"));
        assert!(!text.contains("prod"));
    }

    #[test]
    fn explain_shows_rule_and_source_per_metric() {
        let report = CountReport {
            total: 2,
            metrics: vec![metric(
                "page.views",
                Kind::Count,
                2,
                2,
                "D3",
                "COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).",
            )],
        };
        let text = render_count(&report, true, None, &[], &[], None, None);
        assert!(text.contains(
            "  D3: COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge)."
        ));
        assert!(text.contains(&format!("  source: {CUSTOM_METRICS_SOURCE}")));
        assert!(text.contains(ESTIMATE_LINE));
    }

    #[test]
    fn empty_sample_has_total_zero_and_estimate() {
        let text = render_count(
            &CountReport {
                total: 0,
                metrics: Vec::new(),
            },
            true,
            None,
            &[],
            &[],
            None,
            None,
        );
        assert_eq!(
            text,
            "\
total: 0
estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
"
        );
        assert!(!text.contains("https://"));
    }

    #[test]
    fn allotment_sits_between_total_and_the_metrics() {
        let report = CountReport {
            total: 4,
            metrics: vec![metric(
                "page.views",
                Kind::Count,
                4,
                4,
                "D3",
                "COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).",
            )],
        };
        let allotment = allotment(Plan::Pro, 3, report.total, None).expect("allotment");
        let text = render_count(&report, false, Some(&allotment), &[], &[], None, None);
        assert_eq!(
            text,
            "\
total: 4
allotment: 300
usage: 4 (total, indexed)
surplus: 0

page.views type: count combinations: 4 count: 4 rule: D3

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
"
        );
        assert_eq!(text.matches(ESTIMATE_LINE).count(), 1);
        assert!(!text.contains("https://"));
        assert!(!text.contains("\ningested:"));
        let json = render_count_json(
            &[Path::new("sample.dogstatsd")],
            &report,
            Some(&allotment),
            &[],
            &[],
            None,
            None,
        );
        let doc: Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(doc["allotment"]["usage"], 4);
        assert_eq!(doc["allotment"]["basis"], "total (indexed)");
        assert_eq!(doc["allotment"]["surplus"], 0);
        assert!(doc["allotment"].get("indexed").is_none());
        assert!(doc["allotment"].get("ingested").is_none());
    }

    #[test]
    fn allotment_with_allowlist_sets_indexed_and_ingested_apart() {
        let report = CountReport {
            total: 4,
            metrics: vec![metric(
                "request.Latency",
                Kind::Count,
                4,
                4,
                "D3",
                "COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).",
            )],
        };
        let allotment = allotment(Plan::Pro, 3, 3, Some(4)).expect("allotment");
        let text = render_count(&report, false, Some(&allotment), &[], &[], None, None);
        assert!(text.contains("allotment: 300\n"), "{text}");
        assert!(text.contains("indexed: 3 surplus: 0\n"), "{text}");
        assert!(text.contains("ingested: 4 surplus: 0\n"), "{text}");
        assert!(!text.contains("usage:"), "{text}");
        let json = render_count_json(
            &[Path::new("sample.dogstatsd")],
            &report,
            Some(&allotment),
            &[],
            &[],
            None,
            None,
        );
        let doc: Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(doc["allotment"]["indexed"], 3);
        assert_eq!(doc["allotment"]["indexed_surplus"], 0);
        assert_eq!(doc["allotment"]["ingested"], 4);
        assert_eq!(doc["allotment"]["ingested_surplus"], 0);
        assert!(doc["allotment"].get("usage").is_none());
        assert!(doc["allotment"].get("basis").is_none());
    }

    #[test]
    fn tags_sit_under_the_metric_without_values() {
        let report = CountReport {
            total: 4,
            metrics: vec![metric(
                "checkout",
                Kind::Count,
                4,
                4,
                "D3",
                "COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).",
            )],
        };
        let tags = vec![MetricTags {
            name: "checkout".to_string(),
            kind: Kind::Count,
            keys: vec![
                TagKey {
                    key: "user_id".to_string(),
                    unique_values: 3,
                    added: 2,
                },
                TagKey {
                    key: "env".to_string(),
                    unique_values: 1,
                    added: 0,
                },
            ],
        }];
        let text = render_count(&report, false, None, &tags, &[], None, None);
        assert_eq!(
            text,
            "\
total: 4

checkout type: count combinations: 4 count: 4 rule: D3
  tag: user_id values: 3 added: 2
  tag: env values: 1 added: 0

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
"
        );
        assert!(!text.contains("u1"));
        assert!(!text.contains("prod"));
    }

    #[test]
    fn unbounded_tag_shows_the_reason_without_the_value() {
        let report = CountReport {
            total: 1,
            metrics: vec![metric(
                "checkout",
                Kind::Count,
                1,
                1,
                "D3",
                "COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).",
            )],
        };
        let unbounded = vec![MetricUnbounded {
            name: "checkout".to_string(),
            kind: Kind::Count,
            keys: vec![UnboundedTag {
                key: "user_id".to_string(),
                reasons: vec![UnboundedReason::Threshold, UnboundedReason::NumericId],
            }],
        }];
        let text = render_count(&report, false, None, &[], &unbounded, None, None);
        assert_eq!(
            text,
            "\
total: 1

checkout type: count combinations: 1 count: 1 rule: D3
  unbounded: user_id reason: threshold, pattern numeric-id

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
"
        );
        assert!(!text.contains("123456"));
    }

    #[test]
    fn allowlist_shows_ingested_and_indexed_per_metric() {
        let report = CountReport {
            total: 4,
            metrics: vec![metric(
                "request.Latency",
                Kind::Count,
                4,
                4,
                "D3",
                "COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).",
            )],
        };
        let allow = AllowReport {
            ingested: Some(4),
            indexed: 3,
            metrics: vec![MetricAllow {
                name: "request.Latency".to_string(),
                kind: Kind::Count,
                ingested: Some(4),
                indexed: 3,
            }],
        };
        let text = render_count(&report, false, None, &[], &[], Some(&allow), None);
        assert_eq!(
            text,
            "\
total: 4

request.Latency type: count combinations: 4 count: 4 rule: D3
  ingested: 4 indexed: 3

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
"
        );
        assert!(!text.contains("endpoint:X"));
        assert!(!text.contains("status:200"));
    }

    #[test]
    fn price_shows_source_date_and_cost_above_the_allotment() {
        let report = CountReport {
            total: 4,
            metrics: vec![metric(
                "page.views",
                Kind::Count,
                4,
                4,
                "D3",
                "COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).",
            )],
        };
        let cost = CostReport {
            currency: "TST".to_string(),
            source: "https://example.test/source".to_string(),
            date: "2026-01-02".to_string(),
            lines: vec![
                CostLine {
                    kind: CostKind::Ingested,
                    above: 1,
                    amount: "0.02".to_string(),
                },
                CostLine {
                    kind: CostKind::Indexed,
                    above: 50,
                    amount: "1".to_string(),
                },
            ],
        };
        let text = render_count(&report, false, None, &[], &[], None, Some(&cost));
        assert_eq!(
            text,
            "\
total: 4
price: TST per 100
source: https://example.test/source
date: 2026-01-02
cost ingested: 0.02 TST
cost indexed: 1 TST

page.views type: count combinations: 4 count: 4 rule: D3

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
"
        );
    }

    fn delta(
        name: &str,
        kind: Kind,
        old_count: Option<u64>,
        new_count: u64,
        delta: i128,
        tags: Vec<TagDelta>,
    ) -> MetricDelta {
        MetricDelta {
            name: name.to_string(),
            kind,
            old_count,
            new_count: Some(new_count),
            delta,
            rule: "D3",
            tags,
        }
    }

    #[test]
    fn diff_shows_total_new_and_removed_metric_and_tag() {
        let report = DiffReport {
            old_total: 2,
            new_total: 3,
            delta: 1,
            metrics: vec![
                delta(
                    "checkout",
                    Kind::Count,
                    None,
                    1,
                    1,
                    vec![TagDelta {
                        key: "env".to_string(),
                        old_values: 0,
                        new_values: 1,
                        old_added: 0,
                        new_added: 0,
                        delta: 0,
                    }],
                ),
                MetricDelta {
                    name: "queue.depth".to_string(),
                    kind: Kind::Gauge,
                    old_count: Some(1),
                    new_count: None,
                    delta: -1,
                    rule: "D3",
                    tags: vec![TagDelta {
                        key: "host".to_string(),
                        old_values: 1,
                        new_values: 0,
                        old_added: 0,
                        new_added: 0,
                        delta: 0,
                    }],
                },
            ],
        };
        let text = render_diff(&report);
        assert_eq!(
            text,
            "\
old total: 2
new total: 3
delta: 1

new:
checkout type: count count: 1

removed:
queue.depth type: gauge count: 1

checkout type: count old: 0 new: 1 delta: 1 rule: D3
  tag: env values: 0 to 1 added: 0 to 0 delta: 0
queue.depth type: gauge old: 1 new: 0 delta: -1 rule: D3
  tag: host values: 1 to 0 added: 0 to 0 delta: 0

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
"
        );
        assert_eq!(text.matches(ESTIMATE_LINE).count(), 1);
        assert!(!text.contains("prod"));
    }

    #[test]
    fn diff_without_metrics_has_only_totals_and_estimate() {
        let text = render_diff(&DiffReport {
            old_total: 0,
            new_total: 0,
            delta: 0,
            metrics: Vec::new(),
        });
        assert_eq!(
            text,
            "\
old total: 0
new total: 0
delta: 0
estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
"
        );
        assert!(!text.contains("\nnew:\n"));
        assert!(!text.contains("\nremoved:\n"));
    }

    const ROOT_KEYS: &[&str] = &[
        "schema_version",
        "tool",
        "estimate",
        "source",
        "inputs",
        "total",
        "metrics",
        "tags",
        "unbounded",
        "allotment",
    ];

    fn keys(value: &Value) -> BTreeSet<String> {
        value.as_object().expect("object").keys().cloned().collect()
    }

    fn expected_keys() -> BTreeSet<String> {
        ROOT_KEYS.iter().map(|key| (*key).to_string()).collect()
    }

    #[test]
    fn json_count_has_schema_and_no_conditional_keys() {
        let report = CountReport {
            total: 2,
            metrics: vec![metric(
                "page.views",
                Kind::Count,
                2,
                2,
                "D3",
                "COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).",
            )],
        };
        let tags = vec![MetricTags {
            name: "page.views".to_string(),
            kind: Kind::Count,
            keys: vec![TagKey {
                key: "user_id".to_string(),
                unique_values: 3,
                added: 2,
            }],
        }];
        let unbounded = vec![MetricUnbounded {
            name: "page.views".to_string(),
            kind: Kind::Count,
            keys: vec![UnboundedTag {
                key: "user_id".to_string(),
                reasons: vec![UnboundedReason::Threshold, UnboundedReason::NumericId],
            }],
        }];
        let text = render_count_json(
            &[Path::new("sample.dogstatsd")],
            &report,
            None,
            &tags,
            &unbounded,
            None,
            None,
        );
        let doc: Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(keys(&doc), expected_keys());
        assert_eq!(doc["schema_version"], 1);
        assert_eq!(doc["tool"], "dd-metric-bill");
        assert_eq!(doc["estimate"], ESTIMATE_LINE);
        assert_eq!(doc["source"], CUSTOM_METRICS_SOURCE);
        assert_eq!(doc["inputs"], serde_json::json!(["sample.dogstatsd"]));
        assert_eq!(doc["total"], 2);
        assert_eq!(doc["allotment"], Value::Null);
        assert_eq!(doc["metrics"][0]["rule"], "D3");
        assert_eq!(doc["metrics"][0]["count"], 2);
        assert_eq!(doc["tags"][0]["keys"][0]["key"], "user_id");
        assert_eq!(doc["tags"][0]["keys"][0]["values"], 3);
        assert_eq!(doc["tags"][0]["keys"][0]["added"], 2);
        assert_eq!(
            doc["unbounded"][0]["keys"][0]["reasons"],
            serde_json::json!(["threshold", "pattern numeric-id"])
        );
        assert!(doc["metrics"][0].get("ingested").is_none());
        assert!(doc["metrics"][0].get("indexed").is_none());
    }

    #[test]
    fn json_count_ingested_indexed_and_costs_only_when_provided() {
        let report = CountReport {
            total: 4,
            metrics: vec![metric(
                "request.Latency",
                Kind::Count,
                4,
                4,
                "D3",
                "COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).",
            )],
        };
        let allow = AllowReport {
            ingested: Some(4),
            indexed: 3,
            metrics: vec![MetricAllow {
                name: "request.Latency".to_string(),
                kind: Kind::Count,
                ingested: Some(4),
                indexed: 3,
            }],
        };
        let cost = CostReport {
            currency: "TST".to_string(),
            source: "source-test".to_string(),
            date: "2026-01-02".to_string(),
            lines: vec![CostLine {
                kind: CostKind::Ingested,
                above: 0,
                amount: "0".to_string(),
            }],
        };
        let allotment = allotment(Plan::Pro, 3, 3, Some(4)).expect("allotment");
        let text = render_count_json(
            &[Path::new("a.dogstatsd"), Path::new("b.dogstatsd")],
            &report,
            Some(&allotment),
            &[],
            &[],
            Some(&allow),
            Some(&cost),
        );
        let doc: Value = serde_json::from_str(&text).expect("valid json");
        let mut expected = expected_keys();
        expected.insert("ingested".to_string());
        expected.insert("indexed".to_string());
        expected.insert("costs".to_string());
        assert_eq!(keys(&doc), expected);
        assert_eq!(doc["ingested"], 4);
        assert_eq!(doc["indexed"], 3);
        assert_eq!(doc["metrics"][0]["ingested"], 4);
        assert_eq!(doc["metrics"][0]["indexed"], 3);
        assert_eq!(doc["allotment"]["plan"], "pro");
        assert_eq!(doc["allotment"]["hosts"], 3);
        assert_eq!(doc["allotment"]["allotment"], 300);
        assert_eq!(doc["allotment"]["indexed"], 3);
        assert_eq!(doc["allotment"]["indexed_surplus"], 0);
        assert_eq!(doc["allotment"]["ingested"], 4);
        assert_eq!(doc["allotment"]["ingested_surplus"], 0);
        assert!(doc["allotment"].get("usage").is_none());
        assert!(doc["allotment"].get("basis").is_none());
        assert_eq!(doc["costs"]["currency"], "TST");
        assert_eq!(doc["costs"]["source"], "source-test");
        assert_eq!(doc["costs"]["date"], "2026-01-02");
        assert_eq!(doc["costs"]["lines"][0]["kind"], "ingested");
        assert_eq!(doc["costs"]["lines"][0]["amount"], "0");
        assert_eq!(
            doc["inputs"],
            serde_json::json!(["a.dogstatsd", "b.dogstatsd"])
        );
        assert!(!text.contains("endpoint"));
        assert!(!text.contains("status:200"));
    }

    #[test]
    fn json_without_allowlist_does_not_put_indexed_in_the_output() {
        let report = CountReport {
            total: 4,
            metrics: vec![metric(
                "request.Latency",
                Kind::Count,
                4,
                4,
                "D3",
                "COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).",
            )],
        };
        let allow = AllowReport {
            ingested: None,
            indexed: 4,
            metrics: vec![MetricAllow {
                name: "request.Latency".to_string(),
                kind: Kind::Count,
                ingested: None,
                indexed: 4,
            }],
        };
        let text = render_count_json(
            &[Path::new("sample.dogstatsd")],
            &report,
            None,
            &[],
            &[],
            Some(&allow),
            None,
        );
        let doc: Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(keys(&doc), expected_keys());
        assert!(doc["metrics"][0].get("indexed").is_none());
        assert!(!text.contains("ingested"));
        assert!(!text.contains("indexed"));
    }

    #[test]
    fn json_diff_has_old_and_new_totals_without_tag_values() {
        let report = DiffReport {
            old_total: 2,
            new_total: 3,
            delta: 1,
            metrics: vec![MetricDelta {
                name: "checkout".to_string(),
                kind: Kind::Count,
                old_count: None,
                new_count: Some(1),
                delta: 1,
                rule: "D3",
                tags: vec![TagDelta {
                    key: "env".to_string(),
                    old_values: 0,
                    new_values: 1,
                    old_added: 0,
                    new_added: 0,
                    delta: 0,
                }],
            }],
        };
        let text = render_diff_json(
            Path::new("old.dogstatsd"),
            Path::new("new.dogstatsd"),
            &report,
        );
        let doc: Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(keys(&doc), expected_keys());
        assert_eq!(doc["schema_version"], 1);
        assert_eq!(doc["inputs"]["old"], "old.dogstatsd");
        assert_eq!(doc["inputs"]["new"], "new.dogstatsd");
        assert_eq!(doc["total"]["old"], 2);
        assert_eq!(doc["total"]["new"], 3);
        assert_eq!(doc["total"]["delta"], 1);
        assert_eq!(doc["metrics"][0]["old"], 0);
        assert_eq!(doc["metrics"][0]["new"], 1);
        assert_eq!(doc["metrics"][0]["delta"], 1);
        assert_eq!(doc["tags"][0]["keys"][0]["key"], "env");
        assert_eq!(doc["tags"][0]["keys"][0]["new_values"], 1);
        assert_eq!(doc["unbounded"], serde_json::json!([]));
        assert_eq!(doc["allotment"], Value::Null);
        assert!(doc.get("ingested").is_none());
        assert!(doc.get("indexed").is_none());
        assert!(doc.get("costs").is_none());
        assert!(!text.contains("prod"));
        assert_eq!(text.matches(ESTIMATE_LINE).count(), 1);
    }
}

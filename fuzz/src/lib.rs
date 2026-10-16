//! Shared helpers for the fuzz targets.
//!
//! `FuzzSample` builds a [`Sample`] from arbitrary bytes so the count, tag,
//! diff, and render stages can be fuzzed without going through a text
//! parser first. `MARKER` is a tag value that must never reach rendered
//! output; `assert_output_safe` checks that and that no raw control
//! character other than `\n` reaches text output.

use arbitrary::Arbitrary;
use dd_metric_bill::count::{
    count_by_metric, DEFAULT_HISTOGRAM_AGGREGATES, DEFAULT_HISTOGRAM_PERCENTILES,
};
use dd_metric_bill::diff::diff_samples;
use dd_metric_bill::parse::{Kind, Sample, Tag};
use dd_metric_bill::render::{render_count, render_count_json, render_diff, render_diff_json};
use dd_metric_bill::tags::{tag_breakdown, unbounded_tags, DEFAULT_UNBOUNDED_THRESHOLD};
use std::path::Path;

/// Tag value that must never appear in rendered output (count or diff
/// promise: tag values are not included).
pub const MARKER: &str = "FUZZ_MARKER_VALUE_7f3c";

const MAX_SAMPLES: usize = 64;
const MAX_TAGS: usize = 16;

#[derive(Debug, Clone, Arbitrary)]
pub struct FuzzTag {
    pub key: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Arbitrary)]
pub struct FuzzSample {
    pub name: String,
    pub kind: u8,
    pub tags: Vec<FuzzTag>,
    pub d10: bool,
    pub counter_total: bool,
    /// When true, one more tag (key `marker`) carries [`MARKER`] as its value.
    pub mark_value: bool,
}

fn kind_of(n: u8) -> Kind {
    match n % 5 {
        0 => Kind::Count,
        1 => Kind::Gauge,
        2 => Kind::Histogram,
        3 => Kind::Distribution,
        _ => Kind::Untyped,
    }
}

#[must_use]
pub fn to_samples(specs: &[FuzzSample]) -> Vec<Sample> {
    specs
        .iter()
        .take(MAX_SAMPLES)
        .map(|spec| {
            let mut tags: Vec<Tag> = spec
                .tags
                .iter()
                .take(MAX_TAGS)
                .map(|tag| Tag {
                    key: tag.key.clone(),
                    value: tag.value.clone(),
                })
                .collect();
            if spec.mark_value {
                tags.push(Tag {
                    key: "marker".to_string(),
                    value: Some(MARKER.to_string()),
                });
            }
            Sample {
                name: spec.name.clone(),
                kind: kind_of(spec.kind),
                tags,
                d10: spec.d10,
                counter_total: spec.counter_total,
            }
        })
        .collect()
}

/// The marker tag value never reaches `text`.
pub fn assert_no_marker(label: &str, text: &str) {
    assert!(
        !text.contains(MARKER),
        "{label} leaks a tag value: {text:?}"
    );
}

/// No raw control character other than `\n` reaches text output: `render.rs`
/// promises every control character is escaped there. JSON makes no such
/// promise (`render.rs`: "JSON does not use this function: the raw string
/// stays there") and `serde_json` only escapes the control characters the
/// JSON spec requires (0x00-0x1F); a C1 control or DEL can stay raw and
/// still be valid, parseable JSON, so this check does not apply to JSON.
pub fn assert_text_escaped(label: &str, text: &str) {
    for ch in text.chars() {
        assert!(
            ch == '\n' || !ch.is_control(),
            "{label} has a raw control character {ch:?}: {text:?}"
        );
    }
}

/// Runs `count` end to end and checks the JSON parses and the reported
/// total matches the sum of the per-metric counts.
pub fn run_count_pipeline(samples: &[Sample]) {
    let Ok(report) = count_by_metric(
        samples,
        DEFAULT_HISTOGRAM_AGGREGATES,
        DEFAULT_HISTOGRAM_PERCENTILES,
        false,
    ) else {
        return;
    };
    let tags = tag_breakdown(samples, None);
    let unbounded = unbounded_tags(samples, DEFAULT_UNBOUNDED_THRESHOLD);

    let text = render_count(&report, true, None, &tags, &unbounded, None, None);
    assert_no_marker("count text", &text);
    assert_text_escaped("count text", &text);

    let json_text = render_count_json(
        &[Path::new("fuzz.input")],
        &report,
        None,
        &tags,
        &unbounded,
        None,
        None,
    );
    assert_no_marker("count json", &json_text);
    let value: serde_json::Value =
        serde_json::from_str(&json_text).expect("rendered count json must parse");
    let total = value["total"].as_u64().expect("total is a u64");
    let sum: u64 = value["metrics"]
        .as_array()
        .expect("metrics is an array")
        .iter()
        .map(|metric| metric["count"].as_u64().expect("count is a u64"))
        .sum();
    assert_eq!(total, sum, "total does not match the sum of metric counts");
}

/// Runs `diff` end to end and checks the JSON parses and the reported
/// delta matches the sum of the per-metric deltas.
pub fn run_diff_pipeline(old: &[Sample], new: &[Sample]) {
    let Ok(report) = diff_samples(
        old,
        new,
        DEFAULT_HISTOGRAM_AGGREGATES,
        DEFAULT_HISTOGRAM_PERCENTILES,
        false,
        None,
    ) else {
        return;
    };

    let text = render_diff(&report);
    assert_no_marker("diff text", &text);
    assert_text_escaped("diff text", &text);

    let json_text = render_diff_json(Path::new("old.input"), Path::new("new.input"), &report);
    assert_no_marker("diff json", &json_text);
    let value: serde_json::Value =
        serde_json::from_str(&json_text).expect("rendered diff json must parse");
    let delta = value["total"]["delta"].as_i64().expect("delta is an i64");
    let sum: i64 = value["metrics"]
        .as_array()
        .expect("metrics is an array")
        .iter()
        .map(|metric| metric["delta"].as_i64().expect("delta is an i64"))
        .sum();
    assert_eq!(
        delta, sum,
        "total delta does not match the sum of metric deltas"
    );
}

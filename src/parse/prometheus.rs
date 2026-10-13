use std::collections::BTreeMap;

use super::{Kind, ParseError, Sample, Tag};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Counter,
    Gauge,
    Histogram,
    Summary,
    Untyped,
}

enum Mapped<'a> {
    Counter(&'a str),
    Gauge(&'a str),
    Histogram {
        base: &'a str,
        suffix: Suffix,
    },
    SummaryQuantile(&'a str),
    SummarySum(&'a str),
    SummaryCount(&'a str),
    Untyped(&'a str),
    /// `_total` of a counter family: Datadog name `base.count` (D12, B3).
    CounterTotal(&'a str),
    /// `_created` of a counter family. Do not count: the behavior has not been verified.
    Skip,
}

#[derive(Clone, Copy)]
enum Suffix {
    Bucket,
    Sum,
    Count,
}

#[derive(Clone, Copy)]
enum Fail {
    Shape,
    Name,
    Type,
    DuplicateType,
    Label,
    Value,
    Histogram,
}

impl Fail {
    fn text(self) -> &'static str {
        match self {
            Fail::Shape => "shape is wrong",
            Fail::Name => "name is missing or invalid",
            Fail::Type => "type is missing or unknown",
            Fail::DuplicateType => "type is duplicated",
            Fail::Label => "label is missing or invalid",
            Fail::Value => "value is missing or invalid",
            Fail::Histogram => "histogram sample is missing _bucket, _sum or _count",
        }
    }
}

fn fail(line: usize, reason: Fail) -> ParseError {
    ParseError::new("prometheus", line, reason.text())
}

/// Parses Prometheus text into samples.
///
/// `# TYPE` sets the kind. `counter` becomes count, `gauge` becomes gauge.
/// `untyped` and a sample without `# TYPE` become [`Kind::Untyped`] (D11) and
/// count as 0 by default. A `_count`, `_sum`, or `_bucket` name whose
/// base is a counter or gauge is such a separate metric without a type: the suffix
/// belongs only to a histogram or summary (D10). A `_total` sample of a
/// counter family becomes `name.count` (D12, B3). A `_created` sample of that
/// family does not count. An unknown type is an error.
/// A histogram does not become a Datadog histogram (D10): `_count` and `_sum` become
/// count on `name.count` and `name.sum` (without `le`), `_bucket` becomes count on
/// `name.bucket` (with `le` as a tag). A summary becomes `name.count` and
/// `name.sum` (count, without `quantile`) and `name.quantile` (gauge, with
/// `quantile`). A metric name may contain a dot. `# TYPE` comes before the samples.
/// A UTF-8 BOM at the start is skipped.
/// An identical sample line is kept once: a repeated line does not add a sample.
pub fn parse_prometheus(input: &str) -> Result<Vec<Sample>, ParseError> {
    let input = super::without_bom(input);
    let mut types: BTreeMap<String, Family> = BTreeMap::new();
    let mut samples = Vec::new();
    let mut seen = super::FirstLines::new();
    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(body) = type_body(line) {
            let (name, family) = parse_type_body(body).map_err(|reason| fail(line_no, reason))?;
            if types.contains_key(&name) {
                return Err(fail(line_no, Fail::DuplicateType));
            }
            types.insert(name, family);
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if !seen.is_first(line) {
            continue;
        }
        let (name, tags) = parse_sample(line).map_err(|reason| fail(line_no, reason))?;
        let mapped = resolve(&name, &types).map_err(|reason| fail(line_no, reason))?;
        if let Some(sample) = to_sample(mapped, tags) {
            samples.push(sample);
        }
    }
    Ok(samples)
}

fn type_body(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('#')?.trim_start();
    let rest = rest.strip_prefix("TYPE")?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest.trim_start())
    } else {
        None
    }
}

fn parse_type_body(body: &str) -> Result<(String, Family), Fail> {
    let mut parts = body.split_whitespace();
    let Some(name) = parts.next() else {
        return Err(Fail::Type);
    };
    let Some(kind) = parts.next() else {
        return Err(Fail::Type);
    };
    if parts.next().is_some() {
        return Err(Fail::Shape);
    }
    if !valid_metric_name(name) {
        return Err(Fail::Name);
    }
    let family = match kind {
        "counter" => Family::Counter,
        "gauge" => Family::Gauge,
        "histogram" => Family::Histogram,
        "summary" => Family::Summary,
        "untyped" => Family::Untyped,
        _ => return Err(Fail::Type),
    };
    Ok((name.to_string(), family))
}

fn resolve<'a>(name: &'a str, types: &BTreeMap<String, Family>) -> Result<Mapped<'a>, Fail> {
    if let Some(family) = types.get(name).copied() {
        return match family {
            Family::Counter => Ok(Mapped::Counter(name)),
            Family::Gauge => Ok(Mapped::Gauge(name)),
            Family::Untyped => Ok(Mapped::Untyped(name)),
            Family::Summary => Ok(Mapped::SummaryQuantile(name)),
            Family::Histogram => Err(Fail::Histogram),
        };
    }
    if let Some(base) = without_suffix(name, "_total") {
        if matches!(types.get(base), Some(Family::Counter)) {
            return Ok(Mapped::CounterTotal(base));
        }
    }
    if let Some(base) = without_suffix(name, "_created") {
        if matches!(types.get(base), Some(Family::Counter)) {
            return Ok(Mapped::Skip);
        }
    }
    if let Some((base, suffix)) = strip_suffix(name) {
        if let Some(family) = types.get(base).copied() {
            return match (family, suffix) {
                (Family::Histogram, Suffix::Bucket | Suffix::Sum | Suffix::Count) => {
                    Ok(Mapped::Histogram { base, suffix })
                }
                (Family::Summary, Suffix::Sum) => Ok(Mapped::SummarySum(base)),
                (Family::Summary, Suffix::Count) => Ok(Mapped::SummaryCount(base)),
                _ => Ok(Mapped::Untyped(name)),
            };
        }
    }
    Ok(Mapped::Untyped(name))
}

fn to_sample(mapped: Mapped<'_>, tags: Vec<Tag>) -> Option<Sample> {
    Some(match mapped {
        Mapped::Skip => return None,
        Mapped::Counter(base) => plain(base, Kind::Count, tags),
        Mapped::Gauge(base) => plain(base, Kind::Gauge, tags),
        Mapped::Untyped(base) => plain(base, Kind::Untyped, tags),
        Mapped::CounterTotal(base) => Sample {
            name: format!("{base}.count"),
            kind: Kind::Count,
            tags,
            d10: false,
            counter_total: true,
        },
        Mapped::Histogram { base, suffix } => {
            let (part, without_le) = match suffix {
                Suffix::Count => ("count", true),
                Suffix::Sum => ("sum", true),
                Suffix::Bucket => ("bucket", false),
            };
            Sample {
                name: format!("{base}.{part}"),
                kind: Kind::Count,
                tags: if without_le {
                    without(tags, "le")
                } else {
                    tags
                },
                d10: true,
                counter_total: false,
            }
        }
        Mapped::SummaryQuantile(base) => Sample {
            name: format!("{base}.quantile"),
            kind: Kind::Gauge,
            tags,
            d10: true,
            counter_total: false,
        },
        Mapped::SummarySum(base) => Sample {
            name: format!("{base}.sum"),
            kind: Kind::Count,
            tags: without(tags, "quantile"),
            d10: true,
            counter_total: false,
        },
        Mapped::SummaryCount(base) => Sample {
            name: format!("{base}.count"),
            kind: Kind::Count,
            tags: without(tags, "quantile"),
            d10: true,
            counter_total: false,
        },
    })
}

fn plain(name: &str, kind: Kind, tags: Vec<Tag>) -> Sample {
    Sample {
        name: name.to_string(),
        kind,
        tags,
        d10: false,
        counter_total: false,
    }
}

fn without(tags: Vec<Tag>, key: &str) -> Vec<Tag> {
    tags.into_iter().filter(|tag| tag.key != key).collect()
}

fn without_suffix<'a>(name: &'a str, suffix: &str) -> Option<&'a str> {
    let base = name.strip_suffix(suffix)?;
    if base.is_empty() {
        None
    } else {
        Some(base)
    }
}

fn strip_suffix(name: &str) -> Option<(&str, Suffix)> {
    for (suffix, kind) in [
        ("_bucket", Suffix::Bucket),
        ("_sum", Suffix::Sum),
        ("_count", Suffix::Count),
    ] {
        if let Some(base) = name.strip_suffix(suffix) {
            if !base.is_empty() {
                return Some((base, kind));
            }
        }
    }
    None
}

fn parse_sample(line: &str) -> Result<(String, Vec<Tag>), Fail> {
    let (name, mut rest) = take_metric_name(line)?;
    rest = rest.trim_start();
    let tags = if let Some(after_accolade) = rest.strip_prefix('{') {
        let (tags, after_labels) = parse_labels(after_accolade)?;
        rest = after_labels.trim_start();
        tags
    } else {
        Vec::new()
    };
    if rest.is_empty() || rest.starts_with('#') {
        return Err(Fail::Value);
    }
    let (value, after_value) = take_token(rest)?;
    check_value(value)?;
    let after_time = rest_after_value(after_value.trim_start())?;
    if after_time.is_empty() || after_time.starts_with('#') {
        Ok((name.to_string(), tags))
    } else {
        Err(Fail::Shape)
    }
}

fn rest_after_value(rest: &str) -> Result<&str, Fail> {
    if rest.is_empty() || rest.starts_with('#') {
        return Ok(rest);
    }
    let (token, after_token) = take_token(rest)?;
    check_timestamp(token)?;
    let after_token = after_token.trim_start();
    if after_token.is_empty() || after_token.starts_with('#') {
        Ok(after_token)
    } else {
        Err(Fail::Shape)
    }
}

fn parse_labels(input: &str) -> Result<(Vec<Tag>, &str), Fail> {
    let mut rest = input;
    let mut tags = Vec::new();
    loop {
        rest = rest.trim_start();
        if let Some(after_labels) = rest.strip_prefix('}') {
            return Ok((tags, after_labels));
        }
        let (key, after_name) = take_label_name(rest)?;
        rest = after_name.trim_start();
        let Some(after_is) = rest.strip_prefix('=') else {
            return Err(Fail::Label);
        };
        rest = after_is.trim_start();
        let (value, after_value) = parse_quoted(rest)?;
        let value = if value.is_empty() { None } else { Some(value) };
        tags.push(Tag { key, value });
        rest = after_value.trim_start();
        // `label_pairs = label_pair { "," label_pair } [ "," ]`: the trailing comma is allowed.
        if let Some(after_comma) = rest.strip_prefix(',') {
            rest = after_comma;
            continue;
        }
        if let Some(after_labels) = rest.strip_prefix('}') {
            return Ok((tags, after_labels));
        }
        return Err(Fail::Label);
    }
}

fn parse_quoted(input: &str) -> Result<(String, &str), Fail> {
    let mut rest = input.strip_prefix('"').ok_or(Fail::Label)?;
    let mut out = String::new();
    loop {
        let Some(c) = rest.chars().next() else {
            return Err(Fail::Label);
        };
        if c == '"' {
            return Ok((out, &rest[c.len_utf8()..]));
        }
        if c == '\\' {
            let mut chars = rest.chars();
            chars.next();
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('"') => out.push('"'),
                _ => return Err(Fail::Label),
            }
            rest = chars.as_str();
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
}

fn take_metric_name(input: &str) -> Result<(&str, &str), Fail> {
    take_name(input, true).ok_or(Fail::Name)
}

fn take_label_name(input: &str) -> Result<(String, &str), Fail> {
    let (name, rest) = take_name(input, false).ok_or(Fail::Label)?;
    Ok((name.to_string(), rest))
}

fn take_name(input: &str, metric: bool) -> Option<(&str, &str)> {
    let mut chars = input.chars();
    let first = chars.next()?;
    let start_ok = if metric {
        is_metric_start(first)
    } else {
        is_label_start(first)
    };
    if !start_ok {
        return None;
    }
    let mut len = first.len_utf8();
    for c in chars {
        let ok = if metric {
            is_metric_cont(c)
        } else {
            is_label_cont(c)
        };
        if !ok {
            break;
        }
        len += c.len_utf8();
    }
    Some((&input[..len], &input[len..]))
}

fn valid_metric_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if is_metric_start(c) => chars.all(is_metric_cont),
        _ => false,
    }
}

fn is_metric_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == ':'
}

fn is_label_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_metric_cont(c: char) -> bool {
    is_metric_start(c) || c.is_ascii_digit() || c == '.'
}

fn is_label_cont(c: char) -> bool {
    c.is_ascii_alphabetic() || c.is_ascii_digit() || c == '_'
}

fn take_token(input: &str) -> Result<(&str, &str), Fail> {
    if input.is_empty() {
        return Err(Fail::Value);
    }
    let end = input.find(char::is_whitespace).unwrap_or(input.len());
    if end == 0 {
        return Err(Fail::Value);
    }
    Ok((&input[..end], &input[end..]))
}

fn check_value(token: &str) -> Result<(), Fail> {
    match token.parse::<f64>() {
        Ok(_) => Ok(()),
        Err(_) => Err(Fail::Value),
    }
}

fn check_timestamp(token: &str) -> Result<(), Fail> {
    let digits = token
        .strip_prefix('+')
        .or_else(|| token.strip_prefix('-'))
        .unwrap_or(token);
    if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(())
    } else {
        Err(Fail::Shape)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_prometheus, Kind, Tag};
    use crate::count::{
        count_by_metric, CountReport, DEFAULT_HISTOGRAM_AGGREGATES, DEFAULT_HISTOGRAM_PERCENTILES,
        OPENMETRICS_SOURCE, PROMETHEUS_MAPPING_SOURCE,
    };

    fn part(report: &CountReport, name: &str) -> (u64, u64) {
        let metric = report
            .metrics
            .iter()
            .find(|metric| metric.name == name)
            .unwrap_or_else(|| panic!("lacks {name}"));
        (metric.combinations, metric.count)
    }

    #[test]
    fn counter_gauge_and_label_escape() {
        let input = "\
# HELP page_views synthetic
# TYPE page_views counter
page_views{host=\"a\",path=\"a\\\"b\"} 3 1395066363000
# TYPE queue_depth gauge
queue_depth{le=\"1\",host=\"a\"} +Inf
# TYPE jobs_count counter
jobs_count{host=\"a\"} 1
";
        let samples = parse_prometheus(input).expect("valid sample");
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].name, "page_views");
        assert_eq!(samples[0].kind, Kind::Count);
        assert_eq!(
            samples[0].tags,
            vec![
                Tag {
                    key: "host".to_string(),
                    value: Some("a".to_string()),
                },
                Tag {
                    key: "path".to_string(),
                    value: Some("a\"b".to_string()),
                },
            ]
        );
        assert_eq!(samples[1].kind, Kind::Gauge);
        assert_eq!(samples[1].tags[0].key, "le");
        assert_eq!(samples[2].name, "jobs_count");
        assert_eq!(samples[2].kind, Kind::Count);
    }

    /// Two label combinations and four buckets (`0.1`, `1`, `5`, `+Inf`):
    /// count 2 + sum 2 + bucket 8 = 12. Not the DogStatsD factor of 5.
    #[test]
    fn d10_histogram_two_label_combinations_is_12() {
        let input = "\
# TYPE http.duration histogram
http.duration_bucket{host=\"a\",path=\"/x\",le=\"0.1\"} 1
http.duration_bucket{host=\"a\",path=\"/x\",le=\"1\"} 2
http.duration_bucket{host=\"a\",path=\"/x\",le=\"5\"} 3
http.duration_bucket{host=\"a\",path=\"/x\",le=\"+Inf\"} 4
http.duration_count{host=\"a\",path=\"/x\"} 4
http.duration_sum{host=\"a\",path=\"/x\"} 9
http.duration_bucket{host=\"b\",path=\"/y\",le=\"0.1\"} 1
http.duration_bucket{host=\"b\",path=\"/y\",le=\"1\"} 2
http.duration_bucket{host=\"b\",path=\"/y\",le=\"5\"} 3
http.duration_bucket{host=\"b\",path=\"/y\",le=\"+Inf\"} 4
http.duration_count{host=\"b\",path=\"/y\"} 4
http.duration_sum{host=\"b\",path=\"/y\"} 8
";
        let samples = parse_prometheus(input).expect("valid sample");
        assert_eq!(samples.len(), 12);
        for sample in &samples {
            assert!(sample.d10);
            assert_eq!(sample.kind, Kind::Count);
            let has_le = sample.tags.iter().any(|tag| tag.key == "le");
            match sample.name.as_str() {
                "http.duration.bucket" => assert!(has_le),
                "http.duration.count" | "http.duration.sum" => assert!(!has_le),
                other => panic!("unexpected metric {other}"),
            }
        }
        let report = count_by_metric(
            &samples,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        assert_eq!(part(&report, "http.duration.count"), (2, 2));
        assert_eq!(part(&report, "http.duration.sum"), (2, 2));
        assert_eq!(part(&report, "http.duration.bucket"), (8, 8));
        assert_eq!(report.total, 12);
        assert!(report
            .metrics
            .iter()
            .all(|metric| metric.rule.code == "D10"
                && metric.rule.source == PROMETHEUS_MAPPING_SOURCE));
        let with_other_flags = count_by_metric(&samples, 6, 3, true).expect("count");
        assert_eq!(with_other_flags.total, 12);
    }

    /// Two label combinations and three quantiles: count 2 + sum 2 + quantile 6 = 10.
    #[test]
    fn d10_summary_two_label_combinations_is_10() {
        let input = "\
# TYPE rpc.latency summary
rpc.latency{host=\"a\",method=\"get\",quantile=\"0.5\"} 1
rpc.latency{host=\"a\",method=\"get\",quantile=\"0.9\"} 2
rpc.latency{host=\"a\",method=\"get\",quantile=\"0.99\"} 3
rpc.latency_sum{host=\"a\",method=\"get\"} 6
rpc.latency_count{host=\"a\",method=\"get\"} 3
rpc.latency{host=\"b\",method=\"post\",quantile=\"0.5\"} 1
rpc.latency{host=\"b\",method=\"post\",quantile=\"0.9\"} 2
rpc.latency{host=\"b\",method=\"post\",quantile=\"0.99\"} 3
rpc.latency_sum{host=\"b\",method=\"post\"} 6
rpc.latency_count{host=\"b\",method=\"post\"} 3
";
        let samples = parse_prometheus(input).expect("valid sample");
        assert_eq!(samples.len(), 10);
        for sample in &samples {
            assert!(sample.d10);
            let has_quantile = sample.tags.iter().any(|tag| tag.key == "quantile");
            match sample.name.as_str() {
                "rpc.latency.quantile" => {
                    assert_eq!(sample.kind, Kind::Gauge);
                    assert!(has_quantile);
                }
                "rpc.latency.count" | "rpc.latency.sum" => {
                    assert_eq!(sample.kind, Kind::Count);
                    assert!(!has_quantile);
                }
                other => panic!("unexpected metric {other}"),
            }
        }
        let report = count_by_metric(
            &samples,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        assert_eq!(part(&report, "rpc.latency.count"), (2, 2));
        assert_eq!(part(&report, "rpc.latency.sum"), (2, 2));
        assert_eq!(part(&report, "rpc.latency.quantile"), (6, 6));
        assert_eq!(
            report
                .metrics
                .iter()
                .find(|metric| metric.name == "rpc.latency.quantile")
                .expect("quantile")
                .kind,
            Kind::Gauge
        );
        assert_eq!(report.total, 10);
        assert!(report
            .metrics
            .iter()
            .all(|metric| metric.rule.code == "D10"
                && metric.rule.source == PROMETHEUS_MAPPING_SOURCE));
    }

    /// `# TYPE untyped` and a sample without `# TYPE` count as 0 (D11).
    /// A counter in the same sample still counts.
    #[test]
    fn d11_untyped_and_without_type_count_zero() {
        let input = "\
# TYPE page.views untyped
page.views{host=\"a\"} 1
page.views{host=\"b\"} 2
queue.depth{host=\"a\"} 3
# TYPE ok counter
ok{host=\"a\"} 1
";
        let samples = parse_prometheus(input).expect("valid sample");
        assert_eq!(samples.len(), 4);
        assert!(samples
            .iter()
            .filter(|sample| sample.name != "ok")
            .all(|sample| sample.kind == Kind::Untyped && !sample.d10));
        assert_eq!(
            samples
                .iter()
                .find(|sample| sample.name == "ok")
                .expect("ok")
                .kind,
            Kind::Count
        );
        let report = count_by_metric(
            &samples,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        assert_eq!(report.total, 1);
        assert_eq!(part(&report, "ok"), (1, 1));
        assert_eq!(part(&report, "page.views"), (2, 0));
        assert_eq!(part(&report, "queue.depth"), (1, 0));
        assert!(report.metrics.iter().all(|metric| {
            if metric.kind == Kind::Untyped {
                metric.count == 0
                    && metric.rule.code == "D11"
                    && metric.rule.source == OPENMETRICS_SOURCE
            } else {
                metric.rule.code == "D3"
            }
        }));
        let with_other_flags = count_by_metric(&samples, 6, 3, true).expect("count");
        assert_eq!(with_other_flags.total, 1);
    }

    /// A histogram still counts beside a sample without a type. Only that sample counts as 0.
    #[test]
    fn untyped_beside_histogram_is_skipped() {
        let input = "\
# TYPE http.duration histogram
http.duration_count{host=\"a\"} 1
http.duration_sum{host=\"a\"} 2
http.duration_bucket{host=\"a\",le=\"+Inf\"} 1
los{host=\"a\"} 1
";
        let samples = parse_prometheus(input).expect("valid sample");
        let report = count_by_metric(
            &samples,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        assert_eq!(part(&report, "http.duration.count"), (1, 1));
        assert_eq!(part(&report, "http.duration.sum"), (1, 1));
        assert_eq!(part(&report, "http.duration.bucket"), (1, 1));
        assert_eq!(part(&report, "los"), (1, 0));
        assert_eq!(report.total, 3);
        assert_eq!(
            report
                .metrics
                .iter()
                .find(|metric| metric.name == "los")
                .expect("los")
                .kind,
            Kind::Untyped
        );
    }

    /// `foo` is a counter. `foo_count`, `foo_sum`, and `foo_bucket` are not part of
    /// a histogram: each is a separate metric without a type (D11).
    #[test]
    fn suffix_on_counter_of_gauge_is_separate_metric_without_type() {
        let input = "\
# TYPE foo counter
foo{host=\"a\"} 1
foo_count{host=\"a\"} 2
foo_sum{host=\"a\"} 3
foo_bucket{host=\"a\",le=\"1\"} 4
# TYPE bar gauge
bar{host=\"a\"} 1
bar_count{host=\"a\"} 2
";
        let samples = parse_prometheus(input).expect("valid sample");
        assert_eq!(samples.len(), 6);
        let kinds: Vec<_> = samples
            .iter()
            .map(|sample| (sample.name.as_str(), sample.kind, sample.d10))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("foo", Kind::Count, false),
                ("foo_count", Kind::Untyped, false),
                ("foo_sum", Kind::Untyped, false),
                ("foo_bucket", Kind::Untyped, false),
                ("bar", Kind::Gauge, false),
                ("bar_count", Kind::Untyped, false),
            ]
        );
        let report = count_by_metric(
            &samples,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        assert_eq!(part(&report, "foo"), (1, 1));
        assert_eq!(part(&report, "bar"), (1, 1));
        for name in ["foo_count", "foo_sum", "foo_bucket", "bar_count"] {
            assert_eq!(part(&report, name), (1, 0), "{name}");
            let metric = report
                .metrics
                .iter()
                .find(|metric| metric.name == name)
                .expect(name);
            assert_eq!(metric.kind, Kind::Untyped);
            assert_eq!(metric.rule.code, "D11");
            assert!(
                metric
                    .rule
                    .text
                    .contains("whose base is a counter or gauge"),
                "{}",
                metric.rule.text
            );
        }
        assert_eq!(report.total, 2);
    }

    #[test]
    fn histogram_without_suffix_is_invalid() {
        let input = "# TYPE lat histogram\nlat{host=\"a\"} 1\n";
        let err = parse_prometheus(input).expect_err("expected input_error");
        assert_eq!(err.exit_code(), 2);
        assert_eq!(err.line(), 2);
        let msg = err.to_string();
        assert!(msg.contains("histogram sample"));
        assert!(!msg.contains("lat"));
        assert!(!msg.contains("host"));
    }

    #[test]
    fn three_invalid_lines() {
        assert_invalid(
            "# TYPE page_views nope-canary\n",
            1,
            &["page_views", "nope-canary"],
        );
        // Without value stays this one error. One sample without type is D11, not error.
        assert_invalid(
            "page_views{token=\"canary-42\"}\n",
            1,
            &["page_views", "canary-42"],
        );
        assert_invalid(
            "# TYPE page_views counter\npage_views{host=canary} 1\n",
            2,
            &["canary", "page_views"],
        );
    }

    #[test]
    fn unknown_type_stays_error() {
        assert_invalid("# TYPE x foo\nx 1\n", 1, &["foo", "x 1"]);
    }

    #[test]
    fn type_is_duplicate() {
        let err = parse_prometheus("# TYPE page counter\n# TYPE page counter\n")
            .expect_err("expected input_error");
        assert_eq!(err.exit_code(), 2);
        assert_eq!(err.line(), 2);
        assert!(err.to_string().contains("type is duplicated"));
    }

    fn assert_invalid(input: &str, line_no: usize, forbidden: &[&str]) {
        let err = parse_prometheus(input).expect_err("expected input_error");
        assert_eq!(err.exit_code(), 2);
        assert_eq!(err.line(), line_no);
        let msg = err.to_string();
        assert!(msg.contains(&format!("line {line_no}")), "{msg}");
        for piece in forbidden {
            assert!(!msg.contains(piece), "message leaks {piece:?}: {msg}");
        }
        for line in input.lines().map(str::trim).filter(|line| !line.is_empty()) {
            assert!(!msg.contains(line), "message contains the rule: {msg}");
        }
    }
}

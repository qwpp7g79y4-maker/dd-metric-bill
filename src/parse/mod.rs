mod dogstatsd;
mod jsonl;
mod prometheus;

use std::collections::HashSet;
use std::fmt;

/// Lines already stored.
///
/// A repeated line is one sample. The bill counts unique combinations, and
/// keeping every copy of the same line is not bounded in memory.
#[derive(Default)]
pub(crate) struct FirstLines {
    seen: HashSet<String>,
}

impl FirstLines {
    pub(crate) fn new() -> Self {
        Self {
            seen: HashSet::new(),
        }
    }

    /// `true` the first time `line` is seen.
    ///
    /// A line that is already stored does not allocate another copy.
    pub(crate) fn is_first(&mut self, line: &str) -> bool {
        if self.seen.contains(line) {
            return false;
        }
        self.seen.insert(line.to_string());
        true
    }
}

pub use dogstatsd::parse_dogstatsd;
pub use jsonl::parse_jsonl;
pub use prometheus::parse_prometheus;

/// UTF-8 BOM at the start of a file. Later in the file the character stays.
pub(crate) fn without_bom(input: &str) -> &str {
    input.strip_prefix('\u{feff}').unwrap_or(input)
}

/// Error in a sample. The exit code is always 2.
/// The text names the format and the line number (1-based), never the line itself.
#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    line: usize,
    detail: String,
}

impl ParseError {
    pub(crate) fn new(format: &'static str, line: usize, reason: &'static str) -> Self {
        Self {
            line,
            detail: format!("invalid {format} line {line}: {reason}"),
        }
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn exit_code(&self) -> u8 {
        2
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for ParseError {}

/// One metric from a sample. The value itself does not count toward the bill.
///
/// `d10` is set when the sample comes from a Prometheus histogram or summary
/// and, per D10, was placed on `name.count`, `name.sum`, `name.bucket`, or
/// `name.quantile`. The count then uses rule D10 instead of D3.
///
/// `counter_total` is set when a `_total` sample of an OpenMetrics counter
/// was placed, per D12, on `name.count`. The source is then B3, not B2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub name: String,
    pub kind: Kind,
    pub tags: Vec<Tag>,
    pub d10: bool,
    pub counter_total: bool,
}

/// Billable kind. `ms` comes in as [`Kind::Histogram`], `s` as [`Kind::Gauge`].
///
/// [`Kind::Untyped`] is a Prometheus metric with `# TYPE untyped` or without
/// `# TYPE` (D11). A `_count`, `_sum`, or `_bucket` name whose base
/// is a counter or gauge also comes in here: that base is not a histogram
/// or summary. It counts as 0, unless `--untyped-as` converts it to count
/// or gauge before the count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Count,
    Gauge,
    Histogram,
    Distribution,
    Untyped,
}

/// Tag from the sample. `value` is `None` when the tag has no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub key: String,
    pub value: Option<String>,
}

/// One line carried two different values of the key `host`.
pub(crate) const HOST_CONFLICT: &str = "host has more than one value";

/// `true` when `tags` has two or more different values for the key `host`.
///
/// The key is `host`. `HOST` is a different key. A tag with no value does not
/// count. The same value twice is one host. Other keys may repeat.
pub(crate) fn conflicting_host(tags: &[Tag]) -> bool {
    let mut first: Option<&str> = None;
    for tag in tags.iter().filter(|tag| tag.key == "host") {
        let Some(value) = tag.value.as_deref() else {
            continue;
        };
        if let Some(seen) = first {
            if seen != value {
                return true;
            }
        } else {
            first = Some(value);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{
        parse_dogstatsd, parse_jsonl, parse_prometheus, Kind, ParseError, Sample, HOST_CONFLICT,
    };
    use crate::count::{
        count_by_metric, DEFAULT_HISTOGRAM_AGGREGATES, DEFAULT_HISTOGRAM_PERCENTILES,
    };

    /// B1: four combinations of `request.Latency` (`host`, `endpoint`, `status`).
    /// The fifth line repeats the first, with tags in other order.
    const B1_COUNT_DOG: &str = "\
request.Latency:10|c|#host:A,endpoint:X,status:200
request.Latency:12|c|#host:B,endpoint:X,status:200
request.Latency:40|c|#host:B,endpoint:X,status:400
request.Latency:8|c|#host:B,endpoint:Y,status:200
request.Latency:11|c|#status:200,endpoint:X,host:A
";

    const B1_COUNT_JSONL: &str = r#"
{"metric":"request.Latency","type":"count","tags":["host:A","endpoint:X","status:200"]}
{"metric":"request.Latency","type":"count","tags":["host:B","endpoint:X","status:200"]}
{"metric":"request.Latency","type":"count","tags":["host:B","endpoint:X","status:400"]}
{"metric":"request.Latency","type":"count","tags":["host:B","endpoint:Y","status:200"]}
{"metric":"request.Latency","type":"count","tags":["status:200","endpoint:X","host:A"]}
"#;

    const B1_COUNT_PROM: &str = "\
# TYPE request.Latency counter
request.Latency{host=\"A\",endpoint=\"X\",status=\"200\"} 10
request.Latency{host=\"B\",endpoint=\"X\",status=\"200\"} 12
request.Latency{host=\"B\",endpoint=\"X\",status=\"400\"} 40
request.Latency{host=\"B\",endpoint=\"Y\",status=\"200\"} 8
request.Latency{status=\"200\",endpoint=\"X\",host=\"A\"} 11
";

    const B1_HIST_DOG: &str = "\
request.Latency:10|h|#host:A,endpoint:X,status:200
request.Latency:12|h|#host:B,endpoint:X,status:200
request.Latency:40|h|#host:B,endpoint:X,status:400
request.Latency:8|h|#host:B,endpoint:Y,status:200
request.Latency:11|h|#status:200,endpoint:X,host:A
";

    const B1_HIST_JSONL: &str = r#"
{"metric":"request.Latency","type":"histogram","tags":["host:A","endpoint:X","status:200"]}
{"metric":"request.Latency","type":"histogram","tags":["host:B","endpoint:X","status:200"]}
{"metric":"request.Latency","type":"histogram","tags":["host:B","endpoint:X","status:400"]}
{"metric":"request.Latency","type":"histogram","tags":["host:B","endpoint:Y","status:200"]}
{"metric":"request.Latency","type":"histogram","tags":["status:200","endpoint:X","host:A"]}
"#;

    const B1_HIST_PROM: &str = "\
# TYPE request.Latency histogram
request.Latency_bucket{le=\"10\",host=\"A\",endpoint=\"X\",status=\"200\"} 1
request.Latency_bucket{le=\"+Inf\",host=\"A\",endpoint=\"X\",status=\"200\"} +Inf
request.Latency_sum{host=\"A\",endpoint=\"X\",status=\"200\"} 10
request.Latency_count{host=\"A\",endpoint=\"X\",status=\"200\"} 1
request.Latency_bucket{le=\"10\",host=\"B\",endpoint=\"X\",status=\"200\"} 1
request.Latency_sum{host=\"B\",endpoint=\"X\",status=\"200\"} 12
request.Latency_count{host=\"B\",endpoint=\"X\",status=\"200\"} 1
request.Latency_bucket{le=\"10\",host=\"B\",endpoint=\"X\",status=\"400\"} 1
request.Latency_sum{host=\"B\",endpoint=\"X\",status=\"400\"} 40
request.Latency_count{host=\"B\",endpoint=\"X\",status=\"400\"} 1
request.Latency_bucket{le=\"10\",host=\"B\",endpoint=\"Y\",status=\"200\"} 1
request.Latency_sum{host=\"B\",endpoint=\"Y\",status=\"200\"} 8
request.Latency_count{host=\"B\",endpoint=\"Y\",status=\"200\"} 1
";

    fn core_of(samples: &[Sample]) -> (String, Kind, u64, u64) {
        let report = count_by_metric(
            samples,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        assert_eq!(report.metrics.len(), 1);
        let metric = &report.metrics[0];
        (
            metric.name.clone(),
            metric.kind,
            metric.combinations,
            report.total,
        )
    }

    #[test]
    fn same_count_in_three_formats() {
        let count = ("request.Latency".to_string(), Kind::Count, 4, 4);
        let histogram = ("request.Latency".to_string(), Kind::Histogram, 4, 20);
        assert_eq!(core_of(&parse_dogstatsd(B1_COUNT_DOG).expect("dog")), count);
        assert_eq!(core_of(&parse_jsonl(B1_COUNT_JSONL).expect("jsonl")), count);
        assert_eq!(
            core_of(&parse_prometheus(B1_COUNT_PROM).expect("prometheus")),
            count
        );
        assert_eq!(
            core_of(&parse_dogstatsd(B1_HIST_DOG).expect("dog")),
            histogram
        );
        assert_eq!(
            core_of(&parse_jsonl(B1_HIST_JSONL).expect("jsonl")),
            histogram
        );
        // A Prometheus histogram follows D10 (count, sum, and bucket each count), not the DogStatsD factor of 5.
        // This fixture does not have four buckets on every combination: the buckets count as 5.
        let prom = count_by_metric(
            &parse_prometheus(B1_HIST_PROM).expect("prometheus"),
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("count");
        let per: Vec<_> = prom
            .metrics
            .iter()
            .map(|metric| {
                (
                    metric.name.as_str(),
                    metric.kind,
                    metric.count,
                    metric.rule.code,
                )
            })
            .collect();
        assert_eq!(
            per,
            vec![
                ("request.Latency.bucket", Kind::Count, 5, "D10"),
                ("request.Latency.count", Kind::Count, 4, "D10"),
                ("request.Latency.sum", Kind::Count, 4, "D10"),
            ]
        );
        assert_eq!(prom.total, 13);
    }

    /// One line with two different `host` values is rejected in every format.
    /// The message names the format and the line, never the line text.
    #[test]
    fn two_host_values_on_one_line_are_rejected() {
        assert_host_conflict("dogstatsd", "m:1|c|#host:a,host:b\n", parse_dogstatsd);
        assert_host_conflict(
            "jsonl",
            "{\"metric\":\"m\",\"type\":\"count\",\"tags\":[\"host:a\",\"host:b\"]}\n",
            parse_jsonl,
        );
        assert_host_conflict(
            "prometheus",
            "m{host=\"a\",host=\"b\"} 1\n",
            parse_prometheus,
        );
    }

    fn assert_host_conflict(
        format: &str,
        input: &str,
        parse: fn(&str) -> Result<Vec<Sample>, ParseError>,
    ) {
        let err = parse(input).expect_err(format);
        assert_eq!(err.exit_code(), 2, "{format}");
        assert_eq!(err.line(), 1, "{format}");
        assert_eq!(
            err.to_string(),
            format!("invalid {format} line 1: {HOST_CONFLICT}"),
            "{format}"
        );
        for line in input.lines().map(str::trim).filter(|line| !line.is_empty()) {
            assert!(
                !err.to_string().contains(line),
                "{format} message contains the line {line:?}: {err}"
            );
        }
    }

    #[test]
    fn repeated_host_is_one_host_and_other_keys_may_repeat() {
        let samples = parse_dogstatsd("m:1|c|#host:a,host:a\n").expect("same host");
        assert_eq!(crate::count::unique_hosts(&samples), 1);
        let samples = parse_dogstatsd("m:1|c|#env:a,env:b,host:a\n").expect("other keys");
        assert_eq!(samples[0].tags.len(), 3);
        assert_eq!(crate::count::unique_hosts(&samples), 1);
    }
}

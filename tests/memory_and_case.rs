//! Two guarantees from the review: memory stays a small multiple of the
//! input, and tags are counted the way Datadog stores them (lowercase).

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static SERIAL: AtomicU64 = AtomicU64::new(0);

struct TempPath(PathBuf);

impl TempPath {
    fn text(contents: &str) -> Self {
        let n = SERIAL.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "dd-metric-bill-test-mem-{}-{n}.txt",
            std::process::id()
        ));
        std::fs::write(&path, contents).expect("write sample");
        Self(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().expect("path is utf-8")
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
        .args(args)
        .output()
        .expect("run binary")
}

fn total_of(format: &str, sample: &str) -> String {
    let file = TempPath::text(sample);
    let output = run(&["--format", format, "count", file.path()]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    stdout.lines().next().expect("total line").to_string()
}

/// M1: a sample of about 14 MiB with 200,000 unique series must be counted
/// within a virtual memory cap of 160 MiB (about 11 times the input).
#[cfg(target_os = "linux")]
#[test]
fn counting_a_large_sample_stays_within_a_small_multiple_of_the_input() {
    let mut sample = String::new();
    for i in 0..200_000 {
        sample.push_str(&format!(
            "svc.requests.latency_{}:1|c|#env:prod,host:h{},user:u{i},region:r{}\n",
            i % 50,
            i % 2000,
            i % 7
        ));
    }
    let file = TempPath::text(&sample);
    drop(sample);
    let output = Command::new("sh")
        .arg("-c")
        .arg("ulimit -v 163840; exec \"$0\" count \"$1\"")
        .arg(env!("CARGO_BIN_EXE_dd-metric-bill"))
        .arg(file.path())
        .output()
        .expect("run under ulimit");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.starts_with("total: 200000"), "{}", &stdout[..40]);
}

/// M2: Datadog lowercases tags, so `Env:Prod` and `env:prod` are one series.
#[test]
fn tags_that_differ_only_in_case_are_one_series() {
    assert_eq!(
        total_of(
            "dogstatsd",
            "m:1|c|#Env:Prod,HOST:A\nm:1|c|#env:prod,host:a\n"
        ),
        "total: 1"
    );
    assert_eq!(
        total_of(
            "jsonl",
            "{\"metric\":\"m\",\"type\":\"count\",\"tags\":[\"Env:Prod\"]}\n\
             {\"metric\":\"m\",\"type\":\"count\",\"tags\":[\"env:prod\"]}\n"
        ),
        "total: 1"
    );
    assert_eq!(
        total_of(
            "prometheus",
            "# TYPE m counter\nm{Env=\"Prod\"} 1\nm{env=\"prod\"} 2\n"
        ),
        "total: 1"
    );
}

/// Metric names stay case sensitive in Datadog.
#[test]
fn metric_names_stay_case_sensitive() {
    assert_eq!(
        total_of("dogstatsd", "M:1|c|#a:b\nm:1|c|#a:b\n"),
        "total: 2"
    );
}

/// `HOST:A` and `host:a` are one host for the allotment.
#[test]
fn hosts_differing_only_in_case_are_one_host() {
    let file = TempPath::text("m:1|c|#host:A\nm:1|c|#host:a\n");
    let output = run(&["count", "--plan", "pro", file.path()]);
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    assert!(stdout.contains("allotment: 100"), "{stdout}");
}

#[test]
fn hand_built_tags_count_like_parsed_tags() {
    use dd_metric_bill::count::count_by_metric;
    use dd_metric_bill::parse::{parse_dogstatsd, Kind, Sample, Tag};

    let parsed = parse_dogstatsd("m:1|c|#host:a\n").expect("parse");
    let built = Sample {
        name: "m".into(),
        kind: Kind::Count,
        tags: vec![Tag {
            key: "HOST".into(),
            value: Some("A".into()),
        }],
        d10: false,
        counter_total: false,
    };
    let mut samples = parsed;
    samples.push(built);
    let report = count_by_metric(&samples, 4, 1, false).expect("count");
    assert_eq!(report.total, 1);
}

fn built(key: &str, value: &str) -> dd_metric_bill::parse::Sample {
    use dd_metric_bill::parse::{Kind, Sample, Tag};
    Sample {
        name: "m".into(),
        kind: Kind::Count,
        tags: vec![Tag {
            key: key.into(),
            value: Some(value.into()),
        }],
        d10: false,
        counter_total: false,
    }
}

/// A hand-built `HOST:A` is the host `a`, like a parsed `host:a`.
#[test]
fn hand_built_hosts_count_like_parsed_hosts() {
    use dd_metric_bill::count::unique_hosts;
    use dd_metric_bill::parse::parse_dogstatsd;

    let mut samples = parse_dogstatsd("m:1|c|#host:a\n").expect("parse");
    assert_eq!(unique_hosts(&samples), 1);
    samples.push(built("HOST", "A"));
    samples.push(built("Host", "a"));
    assert_eq!(unique_hosts(&samples), 1);
    assert_eq!(unique_hosts(&[built("HOST", "A")]), 1);
}

/// A titlecase letter (U+01C5) is not uppercase but lowercases to U+01C6.
#[test]
fn hand_built_titlecase_counts_like_parsed_lowercase() {
    use dd_metric_bill::count::count_by_metric;
    use dd_metric_bill::parse::parse_dogstatsd;

    let mut samples = parse_dogstatsd("m:1|c|#k:\u{1c6}\n").expect("parse");
    samples.push(built("k", "\u{1c5}"));
    let report = count_by_metric(&samples, 4, 1, false).expect("count");
    assert_eq!(report.total, 1);
}

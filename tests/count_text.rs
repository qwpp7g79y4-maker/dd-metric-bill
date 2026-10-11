use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Output};

const ESTIMATE: &str =
    "estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)";
const SOURCE: &str = "https://docs.datadoghq.com/account_management/billing/custom_metrics";

const B1_TAG_VALUES: &[&str] = &[
    "host:A",
    "host:B",
    "endpoint:X",
    "endpoint:Y",
    "status:200",
    "status:400",
    "200",
    "400",
];

const MIXED_TAG_VALUES: &[&str] = &["web-1", "web-2", "prod", "checkout", "alice"];

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
}

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_str()
        .expect("path is utf-8")
        .to_string()
}

fn failure(output: &Output) -> String {
    format!(
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn count_args(flags: &[&str], fixtures: &[&str]) -> Vec<String> {
    let mut args = Vec::with_capacity(1 + flags.len() + fixtures.len());
    args.push("count".to_string());
    args.extend(flags.iter().map(|flag| (*flag).to_string()));
    args.extend(fixtures.iter().copied().map(fixture));
    args
}

fn assert_snapshot(name: &str, args: &[impl AsRef<OsStr>]) -> String {
    let output = binary().args(args).output().expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    assert!(
        output.stderr.is_empty(),
        "stderr is not empty:\n{}",
        failure(&output)
    );
    let got = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert_eq!(
        got.matches(ESTIMATE).count(),
        1,
        "estimate line should appear once:\n{got}"
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(name);
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read snapshot {}: {err}", path.display()));
    assert_eq!(got, expected, "snapshot {name}");
    got
}

fn assert_not_tag_values(got: &str, values: &[&str]) {
    for value in values {
        assert!(
            !got.contains(value),
            "output shows tag value {value}:\n{got}"
        );
    }
}

#[test]
fn b1_histogram() {
    let got = assert_snapshot(
        "b1-histogram.txt",
        &count_args(&[], &["b1-histogram.dogstatsd"]),
    );
    assert!(!got.contains("https://"), "{got}");
    assert_not_tag_values(&got, B1_TAG_VALUES);
}

#[test]
fn b1_histogram_explain_shows_rule_and_source() {
    let got = assert_snapshot(
        "b1-histogram-explain.txt",
        &count_args(&["--explain"], &["b1-histogram.dogstatsd"]),
    );
    assert!(got.contains(SOURCE), "{got}");
    assert!(got.contains("rule: D4"), "{got}");
    assert_not_tag_values(&got, B1_TAG_VALUES);
}

#[test]
fn b1_histogram_aggregates_6_percentiles_1() {
    let got = assert_snapshot(
        "b1-histogram-aggregates-6-explain.txt",
        &count_args(
            &[
                "--histogram-aggregates",
                "6",
                "--histogram-percentiles",
                "1",
                "--explain",
            ],
            &["b1-histogram.dogstatsd"],
        ),
    );
    assert!(got.contains(SOURCE), "{got}");
    assert!(got.contains("count: 28"), "{got}");
    assert_not_tag_values(&got, B1_TAG_VALUES);
}

#[test]
fn b1_count() {
    let got = assert_snapshot("b1-count.txt", &count_args(&[], &["b1-count.dogstatsd"]));
    assert!(!got.contains("https://"), "{got}");
    assert!(got.contains("rule: D3"), "{got}");
    assert!(!got.contains("ingested"), "{got}");
    assert!(!got.contains("indexed"), "{got}");
    assert_not_tag_values(&got, B1_TAG_VALUES);
}

#[test]
fn b1_count_allow_tags_4_ingested_3_indexed() {
    let got = assert_count_ok(&count_args(
        &["--allow-tags", "endpoint,status"],
        &["b1-count.dogstatsd"],
    ));
    assert!(
        got.contains("request.Latency type: count combinations: 4 count: 4 rule: D3"),
        "{got}"
    );
    assert!(got.contains("  ingested: 4 indexed: 3\n"), "{got}");
    assert_eq!(got.matches("ingested:").count(), 1, "{got}");
    assert_not_tag_values(&got, B1_TAG_VALUES);
}

#[test]
fn b1_distribution_allow_tags_20_ingested_15_indexed() {
    let got = assert_count_ok(&count_args(
        &["--allow-tags", "endpoint,status"],
        &["b1-distribution.dogstatsd"],
    ));
    assert!(got.contains("count: 20"), "{got}");
    assert!(got.contains("  ingested: 20 indexed: 15\n"), "{got}");
    assert_eq!(got.matches("ingested:").count(), 1, "{got}");
    assert_not_tag_values(&got, B1_TAG_VALUES);
}

#[test]
fn b1_count_and_histogram_together() {
    let got = assert_snapshot(
        "b1-count-and-histogram.txt",
        &count_args(&[], &["b1-count.dogstatsd", "b1-histogram.dogstatsd"]),
    );
    assert!(!got.contains("https://"), "{got}");
    assert_not_tag_values(&got, B1_TAG_VALUES);
}

#[test]
fn b1_distribution() {
    let got = assert_snapshot(
        "b1-distribution.txt",
        &count_args(&[], &["b1-distribution.dogstatsd"]),
    );
    assert!(!got.contains("https://"), "{got}");
    assert!(got.contains("rule: D5"), "{got}");
    assert_not_tag_values(&got, B1_TAG_VALUES);
}

#[test]
fn b1_distribution_percentiles_explain() {
    let got = assert_snapshot(
        "b1-distribution-percentiles-explain.txt",
        &count_args(
            &["--percentiles", "--explain"],
            &["b1-distribution.dogstatsd"],
        ),
    );
    assert!(got.contains(SOURCE), "{got}");
    assert!(got.contains("count: 40"), "{got}");
    assert!(got.contains("p99"), "{got}");
    assert_not_tag_values(&got, B1_TAG_VALUES);
}

#[test]
fn mixed_per_metric() {
    let got = assert_snapshot("mixed.txt", &count_args(&[], &["mixed.dogstatsd"]));
    assert!(!got.contains("https://"), "{got}");
    assert_not_tag_values(&got, MIXED_TAG_VALUES);
}

#[test]
fn mixed_explain_source_per_metric() {
    let got = assert_snapshot(
        "mixed-explain.txt",
        &count_args(&["--explain"], &["mixed.dogstatsd"]),
    );
    assert_eq!(got.matches(SOURCE).count(), 5, "{got}");
    assert_not_tag_values(&got, MIXED_TAG_VALUES);
}

#[test]
fn empty_sample() {
    let got = assert_snapshot("empty.txt", &count_args(&[], &["empty.dogstatsd"]));
    assert!(got.contains("total: 0"), "{got}");
    assert!(!got.contains("https://"), "{got}");
    assert!(!got.contains("allotment:"), "{got}");
}

fn assert_count_ok(args: &[impl AsRef<OsStr>]) -> String {
    let output = binary().args(args).output().expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    assert!(
        output.stderr.is_empty(),
        "stderr is not empty:\n{}",
        failure(&output)
    );
    let got = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert_eq!(got.matches(ESTIMATE).count(), 1, "{got}");
    got
}

fn assert_allotment(got: &str, allotment: u64, usage: u64, surplus: u64) {
    assert!(got.contains(&format!("allotment: {allotment}\n")), "{got}");
    assert!(
        got.contains(&format!("usage: {usage} (total, indexed)\n")),
        "{got}"
    );
    assert!(got.contains(&format!("surplus: {surplus}\n")), "{got}");
    assert!(got.contains(&format!("total: {usage}\n")), "{got}");
    assert!(
        !got.contains("\nindexed:"),
        "without an allowlist there is no separate indexed rule:\n{got}"
    );
    assert!(
        !got.contains("\ningested:"),
        "without an allowlist there is no ingested rule:\n{got}"
    );
}

#[test]
fn pro_with_3_hosts_shows_300() {
    let got = assert_count_ok(&count_args(
        &["--plan", "pro", "--hosts", "3"],
        &["b1-count.dogstatsd"],
    ));
    assert_allotment(&got, 300, 4, 0);
    assert_not_tag_values(&got, B1_TAG_VALUES);
}

#[test]
fn enterprise_with_2_hosts_shows_400() {
    let sample = TempSample::new("page.views:1|c|#host:web-1,env:prod\n");
    let got = assert_count_ok(&[
        "count",
        "--plan",
        "enterprise",
        "--hosts",
        "2",
        sample.as_str(),
    ]);
    assert_allotment(&got, 400, 1, 0);
    assert!(!got.contains("web-1"), "{got}");
    assert!(!got.contains("prod"), "{got}");
}

#[test]
fn hosts_derived_from_the_sample() {
    // b1-count has host A and host B: 2 unique hosts, Pro is 200.
    let got = assert_count_ok(&count_args(&["--plan", "pro"], &["b1-count.dogstatsd"]));
    assert_allotment(&got, 200, 4, 0);
    assert_no_tag_shapes(&got);
}

#[test]
fn mixed_count_and_gauge_in_one_call() {
    let sample = TempSample::new(
        "\
page.views:1|c|#host:a,env:prod
queue.depth:3|g|#host:a
page.views:1|c|#host:b,env:prod
queue.depth:4|g|#host:b
page.views:2|c|#env:prod,host:a
temperature:1|g|#city:x
",
    );
    let got = assert_count_ok(&["count", sample.as_str()]);
    assert!(got.contains("total: 5\n"), "{got}");
    assert!(
        got.contains("page.views type: count combinations: 2 count: 2 rule: D3"),
        "{got}"
    );
    assert!(
        got.contains("queue.depth type: gauge combinations: 2 count: 2 rule: D3"),
        "{got}"
    );
    assert!(
        got.contains("temperature type: gauge combinations: 1 count: 1 rule: D3"),
        "{got}"
    );
    assert_not_tag_values(&got, &["host:a", "host:b", "env:prod", "city:x", "prod"]);
}

#[test]
fn allotment_with_allow_tags_separates_indexed_and_ingested() {
    let got = assert_count_ok(&count_args(
        &[
            "--plan",
            "pro",
            "--hosts",
            "3",
            "--allow-tags",
            "endpoint,status",
        ],
        &["b1-count.dogstatsd"],
    ));
    assert!(got.contains("total: 4\n"), "{got}");
    assert!(got.contains("allotment: 300\n"), "{got}");
    assert!(got.contains("indexed: 3 surplus: 0\n"), "{got}");
    assert!(got.contains("ingested: 4 surplus: 0\n"), "{got}");
    assert!(!got.contains("usage:"), "{got}");
    assert_not_tag_values(&got, B1_TAG_VALUES);

    let json = assert_count_ok(&count_args(
        &[
            "--json",
            "--plan",
            "pro",
            "--hosts",
            "3",
            "--allow-tags",
            "endpoint,status",
        ],
        &["b1-count.dogstatsd"],
    ));
    assert!(json.contains("\"indexed\": 3"), "{json}");
    assert!(json.contains("\"indexed_surplus\": 0"), "{json}");
    assert!(json.contains("\"ingested\": 4"), "{json}");
    assert!(json.contains("\"ingested_surplus\": 0"), "{json}");
    assert!(!json.contains("\"usage\""), "{json}");
    assert!(!json.contains("\"basis\""), "{json}");
    assert_not_tag_values(&json, B1_TAG_VALUES);

    let without = assert_count_ok(&count_args(
        &["--json", "--plan", "pro", "--hosts", "3"],
        &["b1-count.dogstatsd"],
    ));
    assert!(
        without.contains("\"basis\": \"total (indexed)\""),
        "{without}"
    );
    assert!(without.contains("\"usage\": 4"), "{without}");
    assert!(without.contains("\"surplus\": 0"), "{without}");
    assert!(!without.contains("\"indexed\""), "{without}");
    assert!(!without.contains("\"ingested\""), "{without}");
}

#[test]
fn surplus_zero_when_underused() {
    let got = assert_count_ok(&count_args(
        &["--plan", "enterprise", "--hosts", "2"],
        &["b1-histogram.dogstatsd"],
    ));
    assert_allotment(&got, 400, 20, 0);
    assert!(!got.contains("surplus: -"), "{got}");
    assert_no_tag_shapes(&got);
}

fn assert_no_tag_shapes(got: &str) {
    for value in [
        "host:A",
        "host:B",
        "endpoint:X",
        "endpoint:Y",
        "status:200",
        "status:400",
    ] {
        assert!(
            !got.contains(value),
            "output shows tag value {value}:\n{got}"
        );
    }
}

#[test]
fn second_file_invalid_gives_exit_2_without_total() {
    let valid = fixture("b1-count.dogstatsd");
    let broken = TempSample::new("page.views:1|c\nbeta.secret:1|nope|#token:canary\n");
    let output = binary()
        .args(["count", &valid, broken.as_str()])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    assert!(
        output.stdout.is_empty(),
        "no count on input error:\n{}",
        failure(&output)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("line 2"), "{stderr}");
    assert!(!stderr.contains("canary"), "{stderr}");
    assert!(!stderr.contains("beta.secret"), "{stderr}");
    assert!(!stderr.contains(ESTIMATE), "{stderr}");
}

struct TempSample(std::path::PathBuf);

impl TempSample {
    fn new(contents: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("dd-metric-bill-test-count-{nanos}.txt"));
        std::fs::write(&path, contents).expect("write sample");
        Self(path)
    }

    fn as_str(&self) -> &str {
        self.0.to_str().expect("path is utf-8")
    }
}

impl Drop for TempSample {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn tag_lines(got: &str) -> Vec<&str> {
    got.lines()
        .filter(|line| line.starts_with("  tag:"))
        .map(str::trim)
        .collect()
}

#[test]
fn user_id_is_the_largest_contributor() {
    let got = assert_count_ok(&count_args(&[], &["user-id.dogstatsd"]));
    assert!(
        got.contains("checkout type: count combinations: 4 count: 4 rule: D3"),
        "{got}"
    );
    assert_eq!(
        tag_lines(&got),
        vec![
            "tag: user_id values: 3 added: 2",
            "tag: endpoint values: 2 added: 1",
            "tag: env values: 1 added: 0",
        ]
    );
    assert_not_tag_values(&got, &["u1", "u2", "u3", "prod", "pay", "cart"]);
}

#[test]
fn top_1_shows_only_the_largest_key() {
    let got = assert_count_ok(&count_args(&["--top", "1"], &["user-id.dogstatsd"]));
    assert_eq!(tag_lines(&got), vec!["tag: user_id values: 3 added: 2"]);
    assert_not_tag_values(&got, &["u1", "u2", "u3", "prod", "pay", "cart"]);
}

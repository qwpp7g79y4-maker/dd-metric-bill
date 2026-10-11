use std::process::{Command, Output};

/// Parallel tests can read the same clock value (Windows has a coarse clock), so a counter keeps names unique.
static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
}

fn has_token(text: &str, token: &str) -> bool {
    text.lines()
        .any(|line| line.split_whitespace().any(|word| word == token))
}

fn failure(output: &Output) -> String {
    format!(
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn help_shows_options_and_metric_name_pricing() {
    let output = binary().arg("--help").output().expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    let help = String::from_utf8(output.stdout).expect("help is utf-8");

    let mut missing = Vec::new();
    for token in [
        "count",
        "diff",
        "--format",
        "--json",
        "--explain",
        "--hosts",
        "--plan",
        "--histogram-aggregates",
        "--histogram-percentiles",
        "--percentiles",
        "--allow-tags",
        "--prices",
        "--budget",
        "--top",
        "--untyped-as",
    ] {
        if !has_token(&help, token) {
            missing.push(token);
        }
    }
    if !help.contains("Contracts with Metric Name pricing are not supported.") {
        missing.push("Metric Name pricing sentence");
    }
    assert!(missing.is_empty(), "help is missing {missing:?}\n{help}");
    assert!(
        !has_token(&help, "--show-values"),
        "--show-values does not belong in the CLI:\n{help}"
    );
}

#[test]
fn unknown_option_exits_with_3() {
    let output = binary()
        .arg("--not-one-option")
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(3), "{}", failure(&output));
}

#[test]
fn unbounded_uuid_shows_reason_without_value() {
    let uuid = "550e8400-e29b-41d4-a716-446655440000";
    let path = TempSample::new(&format!("m:1|c|#env:prod,request_id:{uuid}\n"));
    let output = binary()
        .args(["count", path.as_str()])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(
        stdout.contains("unbounded: request_id reason: pattern uuid"),
        "{stdout}"
    );
    assert!(!stdout.contains("unbounded: env"), "{stdout}");
    assert!(!stdout.contains(uuid), "{stdout}");
    assert!(!stdout.contains("prod"), "{stdout}");
}

#[test]
fn threshold_flag_applies_in_the_binary() {
    let mut contents = String::new();
    for i in 1..=3 {
        contents.push_str(&format!("m:1|c|#bucket:v{i}\n"));
    }
    let path = TempSample::new(&contents);
    let without = binary()
        .args(["count", path.as_str()])
        .output()
        .expect("binary start");
    assert_eq!(without.status.code(), Some(0), "{}", failure(&without));
    let without_stdout = String::from_utf8(without.stdout).expect("stdout is utf-8");
    assert!(!without_stdout.contains("unbounded"), "{without_stdout}");

    let with = binary()
        .args(["count", "--unbounded-threshold", "2", path.as_str()])
        .output()
        .expect("binary start");
    assert_eq!(with.status.code(), Some(0), "{}", failure(&with));
    let stdout = String::from_utf8(with.stdout).expect("stdout is utf-8");
    assert!(
        stdout.contains("unbounded: bucket reason: threshold"),
        "{stdout}"
    );
    assert!(!stdout.contains("v1"), "{stdout}");
    assert!(!stdout.contains("v2"), "{stdout}");
    assert!(!stdout.contains("v3"), "{stdout}");

    let help = binary().arg("--help").output().expect("binary start");
    let help = String::from_utf8(help.stdout).expect("help is utf-8");
    assert!(has_token(&help, "--unbounded-threshold"), "{help}");
    assert!(help.contains("[default: 50]"), "{help}");
}

#[test]
fn diff_with_one_file_exits_with_3() {
    let output = binary()
        .args(["diff", "old-sample"])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(3), "{}", failure(&output));
}

#[test]
fn invalid_dogstatsd_line_exits_with_2() {
    let path = TempSample::new("page.views:1|c\nbeta.secret:1|nope|#token:canary\n");
    let output = binary()
        .args(["count", path.as_str()])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("line 2"), "{stderr}");
    assert!(!stderr.contains("beta.secret"), "{stderr}");
    assert!(!stderr.contains("canary"), "{stderr}");
    assert!(!stderr.contains("page.views"), "{stderr}");
}

struct TempSample(std::path::PathBuf);

impl TempSample {
    fn new(contents: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let n = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("dd-metric-bill-test-cli-{nanos}-{n}.txt"));
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

#[test]
fn invalid_jsonl_line_exits_with_2() {
    let path = TempSample::new(
        "{\"metric\":\"ok\",\"type\":\"count\"}\n{\"metric\":\"canary-42\",\"type\":\"nope\"}\n",
    );
    let output = binary()
        .args(["count", "--format", "jsonl", path.as_str()])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("line 2"), "{stderr}");
    assert!(!stderr.contains("canary-42"), "{stderr}");
    assert!(!stderr.contains("nope"), "{stderr}");
}

#[test]
fn invalid_prometheus_line_exits_with_2() {
    let path = TempSample::new("# TYPE page_views counter\npage_views{token=\"canary-42\"}\n");
    let output = binary()
        .args(["count", "--format", "prometheus", path.as_str()])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("line 2"), "{stderr}");
    assert!(!stderr.contains("canary-42"), "{stderr}");
    assert!(!stderr.contains("page_views"), "{stderr}");
}

#[test]
fn b1_count_jsonl_and_prometheus_total_4() {
    let jsonl = "\
{\"metric\":\"request.Latency\",\"type\":\"count\",\"tags\":[\"host:A\",\"endpoint:X\",\"status:200\"]}
{\"metric\":\"request.Latency\",\"type\":\"count\",\"tags\":[\"host:B\",\"endpoint:X\",\"status:200\"]}
{\"metric\":\"request.Latency\",\"type\":\"count\",\"tags\":[\"host:B\",\"endpoint:X\",\"status:400\"]}
{\"metric\":\"request.Latency\",\"type\":\"count\",\"tags\":[\"host:B\",\"endpoint:Y\",\"status:200\"]}
";
    let prom = "\
# TYPE request.Latency counter
request.Latency{host=\"A\",endpoint=\"X\",status=\"200\"} 10
request.Latency{host=\"B\",endpoint=\"X\",status=\"200\"} 12
request.Latency{host=\"B\",endpoint=\"X\",status=\"400\"} 40
request.Latency{host=\"B\",endpoint=\"Y\",status=\"200\"} 8
";
    for (format_name, contents) in [("jsonl", jsonl), ("prometheus", prom)] {
        let path = TempSample::new(contents);
        let output = binary()
            .args(["count", "--format", format_name, path.as_str()])
            .output()
            .expect("binary start");
        assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
        let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
        assert!(
            stdout.contains("total: 4\n"),
            "{format_name} does not count 4:\n{stdout}"
        );
        assert!(
            stdout.contains("request.Latency type: count combinations: 4 count: 4"),
            "{format_name}:\n{stdout}"
        );
    }
}

#[test]
fn explain_prometheus_histogram_shows_d10_and_b2() {
    let path = TempSample::new(
        "\
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
",
    );
    let output = binary()
        .args([
            "count",
            "--format",
            "prometheus",
            "--explain",
            path.as_str(),
        ])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(
        stdout.contains("http.duration.count type: count combinations: 2 count: 2 rule: D10"),
        "{stdout}"
    );
    assert!(
        stdout.contains("http.duration.sum type: count combinations: 2 count: 2 rule: D10"),
        "{stdout}"
    );
    assert!(
        stdout.contains("http.duration.bucket type: count combinations: 8 count: 8 rule: D10"),
        "{stdout}"
    );
    assert!(stdout.contains("total: 12\n"), "{stdout}");
    assert!(
        stdout.contains("https://docs.datadoghq.com/integrations/guide/prometheus-metrics/"),
        "{stdout}"
    );
    assert!(stdout.contains("  D10: "), "{stdout}");
}

const OPENMETRICS: &str = "https://docs.datadoghq.com/integrations/openmetrics/";

fn count_prometheus(args: &[&str], contents: &str) -> (i32, String, String) {
    let path = TempSample::new(contents);
    let mut cmd = binary();
    cmd.args(args);
    cmd.arg(path.as_str());
    let output = cmd.output().expect("binary start");
    let code = output.status.code().unwrap_or(255);
    // The JSON output lists the input path; leave it out so a temporary
    // directory name cannot look like a tag value.
    let stdout = String::from_utf8(output.stdout)
        .expect("stdout is utf-8")
        .replace(path.as_str(), "<sample>");
    let stderr = String::from_utf8(output.stderr)
        .expect("stderr is utf-8")
        .replace(path.as_str(), "<sample>");
    (code, stdout, stderr)
}

#[test]
fn d11_untyped_and_without_type_are_skipped() {
    let cases = [
        "\
# TYPE page.views untyped
page.views{host=\"web-a\"} 1
page.views{host=\"web-b\"} 2
",
        "\
page.views{host=\"web-a\"} 1
page.views{host=\"web-b\"} 2
",
    ];
    for contents in cases {
        let (code, text, stderr) =
            count_prometheus(&["count", "--format", "prometheus", "--explain"], contents);
        assert_eq!(code, 0, "stderr={stderr}\nstdout={text}");
        assert!(text.contains("total: 0\n"), "{text}");
        assert!(
            text.contains("page.views type: skipped combinations: 2 count: 0 rule: D11"),
            "{text}"
        );
        assert!(text.contains("  D11: "), "{text}");
        assert!(text.contains(OPENMETRICS), "{text}");
        assert!(!text.contains("web-a") && !text.contains("web-b"), "{text}");

        let (code, json, stderr) =
            count_prometheus(&["count", "--format", "prometheus", "--json"], contents);
        assert_eq!(code, 0, "stderr={stderr}\nstdout={json}");
        assert!(json.contains("\"type\": \"skipped\""), "{json}");
        assert!(json.contains("\"count\": 0"), "{json}");
        assert!(json.contains("\"rule\": \"D11\""), "{json}");
        assert!(json.contains("\"total\": 0"), "{json}");
        assert!(json.contains("skipped"), "{json}");
        assert!(json.contains("B3"), "{json}");
        assert!(!json.contains("web-a") && !json.contains("web-b"), "{json}");
        json_valid(&json);
    }
}

#[test]
fn untyped_as_gauge_counts_as_gauge() {
    let contents = "\
# TYPE ok counter
ok 1
# TYPE page.views untyped
page.views{host=\"web-a\"} 1
page.views{host=\"web-b\"} 1
queue.depth 1
";
    let (code, text, stderr) = count_prometheus(
        &["count", "--format", "prometheus", "--untyped-as", "gauge"],
        contents,
    );
    assert_eq!(code, 0, "stderr={stderr}\nstdout={text}");
    assert!(text.contains("total: 4\n"), "{text}");
    assert!(
        text.contains("ok type: count combinations: 1 count: 1 rule: D3"),
        "{text}"
    );
    assert!(
        text.contains("page.views type: gauge combinations: 2 count: 2 rule: D3"),
        "{text}"
    );
    assert!(
        text.contains("queue.depth type: gauge combinations: 1 count: 1 rule: D3"),
        "{text}"
    );
    assert!(!text.contains("skipped"), "{text}");

    let (code, chosen_count, stderr) = count_prometheus(
        &["count", "--format", "prometheus", "--untyped-as", "count"],
        contents,
    );
    assert_eq!(code, 0, "stderr={stderr}\nstdout={chosen_count}");
    assert!(
        chosen_count.contains("page.views type: count combinations: 2 count: 2 rule: D3"),
        "{chosen_count}"
    );
    assert!(
        chosen_count.contains("queue.depth type: count combinations: 1 count: 1 rule: D3"),
        "{chosen_count}"
    );
    assert!(chosen_count.contains("total: 4\n"), "{chosen_count}");
    assert!(!chosen_count.contains("skipped"), "{chosen_count}");
}

#[test]
fn counter_foo_with_foo_count_is_skipped() {
    let contents = "\
# TYPE foo counter
foo{host=\"a\"} 1
foo_count{host=\"a\"} 2
";
    let (code, text, stderr) =
        count_prometheus(&["count", "--format", "prometheus", "--explain"], contents);
    assert_eq!(code, 0, "stderr={stderr}\nstdout={text}");
    assert!(text.contains("total: 1\n"), "{text}");
    assert!(
        text.contains("foo type: count combinations: 1 count: 1 rule: D3"),
        "{text}"
    );
    assert!(
        text.contains("foo_count type: skipped combinations: 1 count: 0 rule: D11"),
        "{text}"
    );
    assert!(text.contains("whose base is a counter or gauge"), "{text}");
    assert!(text.contains(OPENMETRICS), "{text}");
    assert!(
        !text.contains("host=\"a\"") && !text.contains(":a"),
        "{text}"
    );

    let (code, chosen_gauge, stderr) = count_prometheus(
        &[
            "count",
            "--format",
            "prometheus",
            "--untyped-as",
            "gauge",
            "--explain",
        ],
        contents,
    );
    assert_eq!(code, 0, "stderr={stderr}\nstdout={chosen_gauge}");
    assert!(chosen_gauge.contains("total: 2\n"), "{chosen_gauge}");
    assert!(
        chosen_gauge.contains("foo type: count combinations: 1 count: 1 rule: D3"),
        "{chosen_gauge}"
    );
    assert!(
        chosen_gauge.contains("foo_count type: gauge combinations: 1 count: 1 rule: D3"),
        "{chosen_gauge}"
    );
    assert!(!chosen_gauge.contains("skipped"), "{chosen_gauge}");
}

#[test]
fn hosts_and_histogram_percentiles_too_large_exits_with_3() {
    let path = TempSample::new("latency:1|h|#host:a\npage.views:1|c|#host:a\n");
    let cases = [
        (
            vec![
                "count",
                "--plan",
                "pro",
                "--hosts",
                "18446744073709551615",
                path.as_str(),
            ],
            "allotment does not fit",
        ),
        (
            vec![
                "count",
                "--histogram-percentiles",
                "18446744073709551615",
                path.as_str(),
            ],
            "histogram-percentiles",
        ),
        (
            vec![
                "count",
                "--histogram-aggregates",
                "18446744073709551615",
                path.as_str(),
            ],
            "histogram-aggregates",
        ),
    ];
    for (args, expected) in cases {
        let output = binary().args(&args).output().expect("binary start");
        let code = output.status.code();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(code, Some(3), "stdout={stdout}\nstderr={stderr}");
        assert!(stdout.is_empty(), "{stdout}");
        assert!(!stderr.contains("panicked"), "{stderr}");
        assert!(!stderr.contains("expect"), "{stderr}");
        assert!(stderr.contains("usage error:"), "{stderr}");
        assert!(stderr.contains(expected), "{stderr}");
        assert!(!stderr.contains("host:a"), "{stderr}");
    }
}

#[test]
fn sum_of_histogram_options_names_both_and_the_sum() {
    let path = TempSample::new("latency:1|h|#host:web-1,endpoint:checkout\n");
    let aggregates = "18446744073709551614";
    let percentiles = "2";
    let output = binary()
        .args([
            "count",
            "--histogram-aggregates",
            aggregates,
            "--histogram-percentiles",
            percentiles,
            path.as_str(),
        ])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(3), "{}", failure(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("usage error:"), "{stderr}");
    assert!(stderr.contains("--histogram-aggregates"), "{stderr}");
    assert!(stderr.contains("--histogram-percentiles"), "{stderr}");
    assert!(stderr.contains(aggregates), "{stderr}");
    assert!(stderr.contains(percentiles), "{stderr}");
    assert!(stderr.contains("the sum"), "{stderr}");
    assert!(stderr.contains("too large"), "{stderr}");
    assert!(!stderr.contains("web-1"), "{stderr}");
    assert!(!stderr.contains("checkout"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn product_of_histogram_factor_names_metric_factor_and_combinations() {
    // 9223372036854775807 + 0 fits. Three combinations times that factor do not fit.
    let path = TempSample::new(
        "\
latency:1|h|#host:web-1,endpoint:checkout
latency:1|h|#host:web-2,endpoint:checkout
latency:1|h|#host:web-3,endpoint:login
",
    );
    let factor = "9223372036854775807";
    let output = binary()
        .args([
            "count",
            "--histogram-aggregates",
            factor,
            "--histogram-percentiles",
            "0",
            path.as_str(),
        ])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(3), "{}", failure(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("usage error:"), "{stderr}");
    assert!(stderr.contains("latency"), "{stderr}");
    assert!(stderr.contains(factor), "{stderr}");
    assert!(stderr.contains("3 combinations"), "{stderr}");
    assert!(!stderr.contains("the sum"), "{stderr}");
    for value in ["web-1", "web-2", "web-3", "checkout", "login"] {
        assert!(!stderr.contains(value), "{stderr}");
    }
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn count_without_histogram_ignores_extreme_aggregates() {
    let count = TempSample::new(
        "\
page.views:1|c|#host:a
page.views:1|c|#host:b
",
    );
    let output = binary()
        .args([
            "count",
            "--histogram-aggregates",
            "18446744073709551615",
            count.as_str(),
        ])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.contains("total: 2\n"), "{stdout}");
    assert!(
        stdout.contains("page.views type: count combinations: 2 count: 2 rule: D3"),
        "{stdout}"
    );

    let histogram = TempSample::new("latency:1|h|#host:web-1\n");
    let output = binary()
        .args([
            "count",
            "--histogram-aggregates",
            "18446744073709551615",
            histogram.as_str(),
        ])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(3), "{}", failure(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("usage error:"), "{stderr}");
    assert!(!stderr.contains("web-1"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn unknown_prometheus_type_exits_with_2() {
    let (code, _stdout, stderr) =
        count_prometheus(&["count", "--format", "prometheus"], "# TYPE x foo\nx 1\n");
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("line 1"), "{stderr}");
    assert!(!stderr.contains("foo"), "{stderr}");
    assert!(!stderr.contains("# TYPE"), "{stderr}");
    assert!(!stderr.contains("x 1"), "{stderr}");
}

fn json_valid(text: &str) {
    let mut child = std::process::Command::new("python3")
        // Python on Windows reads stdin as the ANSI code page unless told otherwise.
        .env("PYTHONIOENCODING", "utf-8")
        .args(["-m", "json.tool"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("python3 start");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        use std::io::Write;
        stdin.write_all(text.as_bytes()).expect("write json");
    }
    let output = child.wait_with_output().expect("python3 finished");
    assert!(
        output.status.success(),
        "python3 -m json.tool fails: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

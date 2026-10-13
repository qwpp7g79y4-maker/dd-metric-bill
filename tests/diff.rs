use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Output};

/// Parallel tests can read the same clock value (Windows has a coarse clock), so a counter keeps names unique.
static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

const ESTIMATE: &str =
    "estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)";

const TAG_TEXT: &[&str] = &["host:a", "host:b", "region:eu", "region:us", "env:prod"];
const TAGTOKENS: &[&str] = &["prod", "eu", "us", "a", "b"];

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

fn diff_args(flags: &[&str]) -> Vec<String> {
    let mut args = Vec::with_capacity(3 + flags.len());
    args.push("diff".to_string());
    args.extend(flags.iter().copied().map(str::to_string));
    args.push(fixture("diff-old.dogstatsd"));
    args.push(fixture("diff-new.dogstatsd"));
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

struct TempSample(std::path::PathBuf);

impl TempSample {
    fn new(contents: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let n = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("dd-metric-bill-test-diff-{nanos}-{n}.txt"));
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
fn diff_shows_added_tag_and_removed_metric() {
    let got = assert_snapshot("diff-tag-and-metric.txt", &diff_args(&[]));
    assert!(
        got.contains("new:\ncheckout type: count count: 1\n"),
        "{got}"
    );
    assert!(
        got.contains("removed:\nqueue.depth type: gauge count: 1\n"),
        "{got}"
    );
    assert!(
        got.contains("tag: region values: 0 to 2 added: 0 to 1 delta: 1"),
        "{got}"
    );
    assert!(got.contains("delta: 1\n"), "{got}");
    for value in TAG_TEXT {
        assert!(
            !got.contains(value),
            "output shows tag value {value}:\n{got}"
        );
    }
    for value in TAGTOKENS {
        assert!(
            !has_token(&got, value),
            "output shows tag value {value}:\n{got}"
        );
    }
}

fn has_token(text: &str, token: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word == token)
}

#[test]
fn budget_equal_to_count_total_is_exit_0() {
    let output = binary()
        .args(["count", "--budget", "4", &fixture("b1-count.dogstatsd")])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    assert!(output.stderr.is_empty(), "{}", failure(&output));
    let got = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let expected = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots")
            .join("b1-count.txt"),
    )
    .expect("snapshot");
    assert_eq!(got, expected);
}

#[test]
fn budget_above_count_total_is_exit_1() {
    let output = binary()
        .args(["count", "--budget", "3", &fixture("b1-count.dogstatsd")])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(1), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.contains("total: 4"), "{stdout}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("budget exceeded: 4 > 3"), "{stderr}");
}

#[test]
fn budget_equal_to_new_total_is_exit_0() {
    let got = assert_snapshot("diff-tag-and-metric.txt", &diff_args(&["--budget", "4"]));
    assert!(got.contains("new total: 4"), "{got}");
}

#[test]
fn budget_above_new_total_is_exit_1() {
    let output = binary()
        .args(diff_args(&["--budget", "3"]))
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(1), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.contains("new total: 4"), "{stdout}");
    assert!(stdout.contains("estimate:"), "{stdout}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("budget exceeded: 4 > 3"), "{stderr}");
}

#[test]
fn budget_looks_at_the_new_total() {
    let old = TempSample::new("page.views:1|c|#host:a\npage.views:1|c|#host:b\n");
    let new = TempSample::new("page.views:1|c|#host:a\n");
    let within = binary()
        .args(["diff", "--budget", "1", old.as_str(), new.as_str()])
        .output()
        .expect("binary start");
    assert_eq!(within.status.code(), Some(0), "{}", failure(&within));
    assert!(within.stderr.is_empty(), "{}", failure(&within));

    let above = binary()
        .args(["diff", "--budget", "1", new.as_str(), old.as_str()])
        .output()
        .expect("binary start");
    assert_eq!(above.status.code(), Some(1), "{}", failure(&above));
    let stderr = String::from_utf8_lossy(&above.stderr);
    assert!(stderr.contains("budget exceeded: 2 > 1"), "{stderr}");
}

#[test]
fn budget_zero_on_empty_sample_is_exit_0() {
    let output = binary()
        .args(["count", "--budget", "0", &fixture("empty.dogstatsd")])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    assert!(output.stderr.is_empty(), "{}", failure(&output));
}

#[test]
fn broken_input_stays_exit_2_even_with_budget() {
    let path = TempSample::new("page.views:1|c\ngeheim:1|nope|#token:super_canary\n");
    let output = binary()
        .args([
            "diff",
            "--budget",
            "0",
            &fixture("diff-old.dogstatsd"),
            path.as_str(),
        ])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("line 2"), "{stderr}");
    assert!(!stderr.contains("canary"), "{stderr}");
    assert!(!stderr.contains("super_canary"), "{stderr}");
    assert!(!stderr.contains("page.views"), "{stderr}");
}

#[test]
fn missing_sample_is_exit_2() {
    let output = binary()
        .args(["diff", &fixture("diff-old.dogstatsd"), "not-a-sample.txt"])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot read sample"), "{stderr}");
}

/// Files named `empty` and `hist`, matching the reported command.
struct NamedPair {
    dir: std::path::PathBuf,
}

impl NamedPair {
    fn new(hist: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let n = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("dd-metric-bill-test-diff-pair-{nanos}-{n}"));
        std::fs::create_dir(&dir).expect("temp dir");
        std::fs::write(dir.join("empty"), "").expect("empty sample");
        std::fs::write(dir.join("hist"), hist).expect("hist sample");
        Self { dir }
    }
}

impl Drop for NamedPair {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn diff_histogram(aggregates: &str, hist: &str) -> Output {
    let pair = NamedPair::new(hist);
    binary()
        .current_dir(&pair.dir)
        .args([
            "diff",
            "--histogram-aggregates",
            aggregates,
            "--histogram-percentiles",
            "0",
            "empty",
            "hist",
        ])
        .output()
        .expect("binary start")
}

#[test]
fn diff_histogram_difference_outside_i64_is_not_101() {
    let one = diff_histogram("9223372036854775808", "latency:1|h|#host:a\n");
    assert_ne!(one.status.code(), Some(101), "{}", failure(&one));
    assert_eq!(one.status.code(), Some(0), "{}", failure(&one));
    let one_out = String::from_utf8(one.stdout).expect("stdout is utf-8");
    assert!(one_out.contains("old total: 0\n"), "{one_out}");
    assert!(
        one_out.contains("new total: 9223372036854775808\n"),
        "{one_out}"
    );
    assert!(
        one_out.contains("delta: 9223372036854775808\n"),
        "{one_out}"
    );

    let two = diff_histogram(
        "4611686018427387904",
        "latency:1|h|#host:a\nlatency:1|h|#host:b\n",
    );
    assert_ne!(two.status.code(), Some(101), "{}", failure(&two));
    assert_eq!(two.status.code(), Some(0), "{}", failure(&two));
    let two_out = String::from_utf8(two.stdout).expect("stdout is utf-8");
    assert!(two_out.contains("old total: 0\n"), "{two_out}");
    assert!(
        two_out.contains("new total: 9223372036854775808\n"),
        "{two_out}"
    );
    assert!(
        two_out.contains("delta: 9223372036854775808\n"),
        "{two_out}"
    );
}

#[test]
fn usage_error_is_exit_3() {
    let one = binary()
        .args(["diff", "only-old"])
        .output()
        .expect("binary start");
    assert_eq!(one.status.code(), Some(3), "{}", failure(&one));

    let budget = binary()
        .args(["count", "--budget"])
        .output()
        .expect("binary start");
    assert_eq!(budget.status.code(), Some(3), "{}", failure(&budget));
}

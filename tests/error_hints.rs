//! W-24: every error path says what went wrong (line 1) and what to do next
//! (`hint:` line 2). Exit codes stay: 1 budget, 2 input, 3 usage.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn run(args: &[&str], stdin: &[u8]) -> (Option<i32>, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut pipe = child.stdin.take().expect("stdin");
    let _ = pipe.write_all(stdin);
    drop(pipe);
    let o = child.wait_with_output().expect("wait");
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// Exit `code`, then a one-line message containing `what` and a non-empty hint line.
fn check_stderr(code: Option<i32>, stderr: &str, want: i32, what: &str) {
    assert_eq!(code, Some(want), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 2, "{stderr:?}");
    assert!(lines[0].contains(what), "{stderr:?}");
    let hint = lines[1].strip_prefix("hint: ").unwrap_or("");
    assert!(hint.len() > 10, "{stderr:?}");
}

fn check(args: &[&str], stdin: &[u8], want: i32, what: &str) {
    let mut full = vec!["--verbose"];
    full.extend_from_slice(args);
    let (code, stdout, stderr) = run(&full, stdin);
    assert!(stdout.is_empty(), "{args:?}: {stdout}");
    check_stderr(code, &stderr, want, what);
}

struct Temp(PathBuf);
impl Temp {
    fn new(tag: &str, text: &str) -> Temp {
        let path = std::env::temp_dir().join(format!("ddmb-hints-{}-{tag}", std::process::id()));
        std::fs::write(&path, text).expect("write temp");
        Temp(path)
    }
    fn s(&self) -> &str {
        self.0.to_str().expect("utf-8 path")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn unreadable_inputs_say_what_to_check() {
    check(
        &["count", "/nonexistent/ddmb-hints.dogstatsd"],
        b"",
        2,
        "cannot read sample",
    );
    check(&["count", "/"], b"", 2, "not a regular file");
    check(&["count", "-"], b"\xff\xfe", 2, "cannot read sample: -");
    let ok = fixture("quickstart.dogstatsd");
    check(
        &["count", "--prices", "/nonexistent/ddmb-prices.json", &ok],
        b"",
        2,
        "cannot read price file",
    );
}

#[test]
fn invalid_content_says_how_to_fix_it() {
    check(
        &["count", &fixture("invalid-line.dogstatsd")],
        b"",
        2,
        "invalid dogstatsd line",
    );
    check(
        &["count", "--format", "jsonl", &fixture("invalid-line.jsonl")],
        b"",
        2,
        "invalid jsonl line",
    );
    check(
        &[
            "count",
            "--format",
            "prometheus",
            &fixture("invalid-line.prom"),
        ],
        b"",
        2,
        "line",
    );
    let ok = fixture("quickstart.dogstatsd");
    for bad in [
        "prices-invalid.txt",
        "prices-negative.json",
        "prices-too-large.json",
    ] {
        check(
            &["count", "--prices", &fixture(bad), &ok],
            b"",
            2,
            "invalid price file",
        );
    }
}

#[test]
fn duplicate_price_key_says_which_key_to_remove() {
    let prices = Temp::new("dup", "{\"a\":1,\"a\":2}");
    let ok = fixture("quickstart.dogstatsd");
    let (code, stdout, stderr) = run(&["--verbose", "count", "--prices", prices.s(), &ok], b"");
    assert!(stdout.is_empty(), "{stdout}");
    assert_eq!(code, Some(2), "{stderr}");
    let (what, hint) = stderr.split_once("hint: ").expect("hint line");
    assert!(what.contains('a') && !hint.trim().is_empty(), "{stderr:?}");
}

#[test]
fn budget_exceeded_says_what_to_reduce() {
    let ok = fixture("quickstart.dogstatsd");
    let (code, _stdout, stderr) = run(&["--verbose", "count", "--budget", "0", &ok], b"");
    check_stderr(code, &stderr, 1, "budget exceeded");
}

#[test]
fn counting_overflow_is_a_usage_error_with_hint() {
    let sample = Temp::new("overflow", "m:1|h|#host:a\nm:1|h|#host:b\n");
    check(
        &[
            "--histogram-aggregates",
            "9223372036854775808",
            "--histogram-percentiles",
            "0",
            "count",
            sample.s(),
        ],
        b"",
        3,
        "usage error",
    );
}

#[test]
fn clap_usage_errors_exit_three() {
    for args in [
        &["count"][..],
        &["--format", "nope", "count", "x"][..],
        &["nope"][..],
    ] {
        let (code, stdout, stderr) = run(args, b"");
        assert_eq!(code, Some(3), "{args:?}: {stderr}");
        assert!(stdout.is_empty(), "{args:?}: {stdout}");
        assert!(stderr.contains("error:"), "{args:?}: {stderr}");
        assert!(
            stderr.contains("Usage:") || stderr.contains("--help"),
            "{args:?}: {stderr}"
        );
    }
}

#[cfg(target_os = "linux")]
fn with_full_stdout(args: &[&str]) -> (Option<i32>, String) {
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .expect("/dev/full");
    let o = Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(full))
        .stderr(Stdio::piped())
        .output()
        .expect("run");
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[cfg(target_os = "linux")]
#[test]
fn full_stdout_says_how_to_recover() {
    if !std::path::Path::new("/dev/full").exists() {
        return;
    }
    let ok = fixture("quickstart.dogstatsd");
    let (code, stderr) = with_full_stdout(&["--verbose", "count", &ok]);
    check_stderr(code, &stderr, 2, "could not write the report");
    let (code, stderr) = with_full_stdout(&["--verbose", "--help"]);
    check_stderr(code, &stderr, 3, "could not write the message");
}

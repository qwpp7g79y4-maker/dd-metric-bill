//! A JSON key repeated at one object level is rejected.
//!
//! The message names the format and the line, or the price file, and does not
//! include the raw text. The same name inside a nested value is allowed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static SERIAL: AtomicU64 = AtomicU64::new(0);

struct TempPath(PathBuf);

impl TempPath {
    fn text(contents: &str) -> Self {
        let n = SERIAL.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("dd-metric-bill-test-dup-{nanos}-{n}.txt"));
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

fn failure(output: &Output) -> String {
    format!(
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
        .args(args)
        .output()
        .expect("binary start")
}

fn empty_sample() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/empty.dogstatsd")
        .to_str()
        .expect("path is utf-8")
        .to_string()
}

fn assert_stderr(output: &Output, expected: &str, body: &str, call: &str) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "{call} {body:?}\n{}",
        failure(output)
    );
    assert!(
        output.stdout.is_empty(),
        "{call} {body:?}\n{}",
        failure(output)
    );
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr is utf-8");
    assert_eq!(stderr, expected, "{call} {body:?}");
    for line in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
        assert!(
            !stderr.contains(line),
            "{call} stderr contains the line {line:?}: {stderr}"
        );
    }
}

fn assert_jsonl_duplicate(body: &str, line_no: usize) {
    let bad = TempPath::text(body);
    let good = TempPath::text("{\"metric\":\"ok\",\"type\":\"count\"}\n");
    let expected = format!("invalid jsonl line {line_no}: duplicate key\n");
    let bad_path = bad.path();
    let good_path = good.path();
    let calls: Vec<Vec<&str>> = vec![
        vec!["count", "--format", "jsonl", bad_path],
        vec!["count", "--json", "--format", "jsonl", bad_path],
        vec!["diff", "--format", "jsonl", good_path, bad_path],
        vec!["diff", "--format", "jsonl", bad_path, good_path],
        vec!["diff", "--json", "--format", "jsonl", good_path, bad_path],
        vec!["diff", "--json", "--format", "jsonl", bad_path, good_path],
    ];
    for call in calls {
        let output = run(&call);
        assert_stderr(&output, &expected, body, &call.join(" "));
    }
}

fn assert_price_duplicate(body: &str) {
    let price = TempPath::text(body);
    let sample = empty_sample();
    let price_path = price.path();
    let calls: Vec<Vec<&str>> = vec![
        vec!["count", "--prices", price_path, sample.as_str()],
        vec!["count", "--json", "--prices", price_path, sample.as_str()],
    ];
    for call in calls {
        let output = run(&call);
        assert_stderr(
            &output,
            "price file has a duplicate key\n",
            body,
            &call.join(" "),
        );
    }
}

#[test]
fn jsonl_literal_duplicate_type_exits_2() {
    assert_jsonl_duplicate(r#"{"metric":"m","type":"count","type":"histogram"}"#, 1);
}

#[test]
fn jsonl_duplicate_key_variants_exit_2() {
    let line1 = [
        r#"{"metric":"m","type":"histogram","type":"count"}"#,
        r#"{"metric":"m","metric":"n","type":"count"}"#,
        r#"{"metric":"n","metric":"m","type":"count"}"#,
        r#"{"metric":"m","type":"count","tags":["host:a"],"tags":[]}"#,
        r#"{"metric":"m","type":"count","tags":[],"tags":["host:a"]}"#,
        r#"{"metric":"m","type":"count","value":1,"value":2}"#,
        r#"{"metric":"m","type":"count","value":2,"value":1}"#,
        r#"{"metric":"m","type":"count","extra":1,"extra":2}"#,
        r#"{"metric":"m","type":"count","extra":2,"extra":1}"#,
        "{\"metric\":\"m\",\"type\":\"count\",\"typ\\u0065\":\"histogram\"}",
        "{\"metric\":\"m\",\"typ\\u0065\":\"histogram\",\"type\":\"count\"}",
        "{\t\"metric\"\t:\t\"m\",\t\"type\"\t:\t\"count\",\t\"type\"\t:\t\"histogram\"\t}",
        "{\n\t\"metric\"\t:\t\"m\",\n\t\"type\"\t:\t\"count\",\n\t\"type\"\t:\t\"histogram\"\n}",
    ];
    for body in line1 {
        assert_jsonl_duplicate(body, 1);
    }
    assert_jsonl_duplicate(
        "{\"metric\":\"ok\",\"type\":\"count\"}\n{\"metric\":\"m\",\"type\":\"count\",\"type\":\"histogram\"}\n",
        2,
    );
}

#[test]
fn jsonl_nested_repeat_is_counted() {
    let bodies = [
        "{\"metric\":\"m\",\"type\":\"count\",\"value\":{\"type\":\"histogram\",\"type\":\"count\"}}\n",
        "{\"metric\":\"m\",\"type\":\"count\",\"value\":{\"metric\":\"n\"}}\n",
        "{\"metric\":\"m\",\"typ\\u0065\":\"count\"}\n",
    ];
    for body in bodies {
        let path = TempPath::text(body);
        let output = run(&["count", "--format", "jsonl", path.path()]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{body:?}\n{}",
            failure(&output)
        );
        assert!(output.stderr.is_empty(), "{body:?}\n{}", failure(&output));
        let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
        assert!(stdout.contains("total: 1\n"), "{body:?}\n{stdout}");

        let again = TempPath::text(body);
        let json = run(&["count", "--json", "--format", "jsonl", again.path()]);
        assert_eq!(json.status.code(), Some(0), "{body:?}\n{}", failure(&json));
        let doc = String::from_utf8(json.stdout).expect("stdout is utf-8");
        assert!(doc.contains("\"total\": 1"), "{body:?}\n{doc}");
    }
}

#[test]
fn price_literal_duplicate_currency_exits_2() {
    assert_price_duplicate(
        r#"{"currency":"USD","currency":"EUR","source":"s","date":"2026-10-07","ingested_per_100":1}"#,
    );
}

#[test]
fn price_duplicate_key_variants_exit_2() {
    let bodies = [
        r#"{"currency":"EUR","currency":"USD","source":"s","date":"2026-10-07","ingested_per_100":1}"#,
        r#"{"currency":"USD","source":"a","source":"b","date":"2026-10-07","ingested_per_100":1}"#,
        r#"{"currency":"USD","source":"b","source":"a","date":"2026-10-07","ingested_per_100":1}"#,
        r#"{"currency":"USD","source":"s","date":"2026-10-07","date":"2026-10-08","ingested_per_100":1}"#,
        r#"{"currency":"USD","source":"s","date":"2026-10-08","date":"2026-10-07","ingested_per_100":1}"#,
        r#"{"currency":"USD","source":"s","date":"2026-10-07","ingested_per_100":1,"ingested_per_100":2}"#,
        r#"{"currency":"USD","source":"s","date":"2026-10-07","ingested_per_100":2,"ingested_per_100":1}"#,
        r#"{"currency":"USD","source":"s","date":"2026-10-07","ingested_per_100":1,"indexed_per_100":3,"indexed_per_100":4}"#,
        r#"{"currency":"USD","source":"s","date":"2026-10-07","ingested_per_100":1,"indexed_per_100":4,"indexed_per_100":3}"#,
        r#"{"currency":"USD","source":"s","date":"2026-10-07","ingested_per_100":1,"extra":1,"extra":2}"#,
        r#"{"currency":"USD","source":"s","date":"2026-10-07","ingested_per_100":1,"extra":2,"extra":1}"#,
        "{\"currency\":\"USD\",\"currenc\\u0079\":\"EUR\",\"source\":\"s\",\"date\":\"2026-10-07\",\"ingested_per_100\":1}",
        "{\"currenc\\u0079\":\"EUR\",\"currency\":\"USD\",\"source\":\"s\",\"date\":\"2026-10-07\",\"ingested_per_100\":1}",
        "{\n\t\"currency\"\t:\t\"USD\",\n\t\"currency\"\t:\t\"EUR\",\n\t\"source\"\t:\t\"s\",\n\t\"date\"\t:\t\"2026-10-07\",\n\t\"ingested_per_100\"\t:\t1\n}\n",
    ];
    for body in bodies {
        assert_price_duplicate(body);
    }
}

#[test]
fn price_nested_repeat_is_an_unknown_field() {
    let body = r#"{"currency":"USD","source":"s","date":"2026-10-07","ingested_per_100":1,"extra":{"currency":"EUR","currency":"USD"}}"#;
    let price = TempPath::text(body);
    let sample = empty_sample();
    let output = run(&["count", "--prices", price.path(), sample.as_str()]);
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    assert!(output.stdout.is_empty(), "{}", failure(&output));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert!(stderr.contains("unknown field"), "{stderr}");
    assert!(!stderr.contains("duplicate key"), "{stderr}");
    assert!(!stderr.contains(body), "{stderr}");
}

#[test]
fn whitespace_without_a_duplicate_is_read() {
    let body = "{\n\t\"currency\"\t:\t\"USD\",\n\t\"source\"\t:\t\"s\",\n\t\"date\"\t:\t\"2026-10-07\",\n\t\"ingested_per_100\"\t:\t1\n}\n";
    let price = TempPath::text(body);
    let sample = empty_sample();
    let output = run(&["count", "--prices", price.path(), sample.as_str()]);
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    assert!(output.stderr.is_empty(), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.contains("total: 0\n"), "{stdout}");
}

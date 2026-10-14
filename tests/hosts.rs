//! One line with two different `host` values is rejected.
//!
//! The same value twice is one host. Other keys may repeat.
//! The key is `host`; `HOST` is a different key.

use std::path::PathBuf;
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
        let path = std::env::temp_dir().join(format!("dd-metric-bill-test-hosts-{nanos}-{n}.txt"));
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

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
}

fn failure(output: &Output) -> String {
    format!(
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn run(args: &[String]) -> Output {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    binary().args(&refs).output().expect("binary start")
}

fn good_sample(format_name: &str) -> &'static str {
    match format_name {
        "dogstatsd" => "m:1|c|#host:a\n",
        "jsonl" => "{\"metric\":\"m\",\"type\":\"count\",\"tags\":[\"host:a\"]}\n",
        "prometheus" => "m{host=\"a\"} 1\n",
        other => panic!("unknown format {other}"),
    }
}

/// Every variant exits 2 with the same reason. Stdout stays empty, and the
/// message does not contain the input line.
fn assert_conflict(format_name: &str, body: &str, line_no: usize, also_default: bool) {
    let bad = TempPath::text(body);
    let good = TempPath::text(good_sample(format_name));
    let expected = format!("invalid {format_name} line {line_no}: host has more than one value\n");
    let bad_path = bad.path().to_string();
    let good_path = good.path().to_string();
    let format = format_name.to_string();
    let mut calls = vec![
        vec![
            "count".to_string(),
            "--format".to_string(),
            format.clone(),
            bad_path.clone(),
        ],
        vec![
            "count".to_string(),
            "--format".to_string(),
            format.clone(),
            "--plan".to_string(),
            "pro".to_string(),
            bad_path.clone(),
        ],
        vec![
            "count".to_string(),
            "--format".to_string(),
            format.clone(),
            "--plan".to_string(),
            "pro".to_string(),
            "--hosts".to_string(),
            "4".to_string(),
            bad_path.clone(),
        ],
        vec![
            "diff".to_string(),
            "--format".to_string(),
            format.clone(),
            good_path.clone(),
            bad_path.clone(),
        ],
        vec![
            "diff".to_string(),
            "--format".to_string(),
            format,
            bad_path.clone(),
            good_path.clone(),
        ],
    ];
    if also_default {
        calls.push(vec!["count".to_string(), bad_path.clone()]);
        calls.push(vec![
            "count".to_string(),
            "--plan".to_string(),
            "pro".to_string(),
            bad_path.clone(),
        ]);
        calls.push(vec![
            "count".to_string(),
            "--plan".to_string(),
            "pro".to_string(),
            "--hosts".to_string(),
            "4".to_string(),
            bad_path.clone(),
        ]);
        calls.push(vec![
            "diff".to_string(),
            good_path.clone(),
            bad_path.clone(),
        ]);
        calls.push(vec!["diff".to_string(), bad_path, good_path]);
    }
    for call in calls {
        let output = run(&call);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{call:?}\n{}",
            failure(&output)
        );
        assert!(output.stdout.is_empty(), "{call:?}\n{}", failure(&output));
        let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
        assert_eq!(stderr, expected, "{call:?}");
        for line in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
            assert!(
                !stderr.contains(line),
                "{call:?} stderr contains the line {line:?}: {stderr}"
            );
        }
    }
}

#[test]
fn two_different_hosts_on_one_line_exit_2() {
    let cases = [
        ("dogstatsd", "m:1|c|#host:a,host:b\n", 1, true),
        ("dogstatsd", "m:1|c|#host:a, host:b\n", 1, false),
        ("dogstatsd", "m:1|c|#host: a,host:b\n", 1, false),
        ("dogstatsd", "m:1|c|# host:a,host:b\n", 1, false),
        ("dogstatsd", "m:1|c|#host:b,host:a\n", 1, false),
        ("dogstatsd", "m:1|c|#host:a,host:b,host:c\n", 1, false),
        ("dogstatsd", "m:1|c|@0.5|#host:a,host:b\n", 1, false),
        (
            "dogstatsd",
            "m:1|c|#host:a\nm:1|c|#host:a,host:b\n",
            2,
            false,
        ),
        (
            "jsonl",
            "{\"metric\":\"m\",\"type\":\"count\",\"tags\":[\"host:a\",\"host:b\"]}\n",
            1,
            false,
        ),
        (
            "jsonl",
            "{\"metric\":\"m\",\"type\":\"count\",\"tags\":[\" host:a\",\"host: b\"]}\n",
            1,
            false,
        ),
        (
            "jsonl",
            "{\"metric\":\"m\",\"type\":\"count\",\"tags\":[\"host:b\",\"host:a\"]}\n",
            1,
            false,
        ),
        (
            "jsonl",
            "{\"metric\":\"m\",\"type\":\"count\",\"tags\":[\"host:a\"]}\n{\"metric\":\"m\",\"type\":\"count\",\"tags\":[\"host:a\",\"host:b\"]}\n",
            2,
            false,
        ),
        ("prometheus", "m{host=\"a\",host=\"b\"} 1\n", 1, false),
        ("prometheus", "m{host=\"a\", host=\"b\"} 1\n", 1, false),
        (
            "prometheus",
            "m{ host = \"a\" , host = \"b\" } 1\n",
            1,
            false,
        ),
        ("prometheus", "m{host=\"b\",host=\"a\"} 1\n", 1, false),
        (
            "prometheus",
            "m{host=\"a\"} 1\nm{host=\"a\",host=\"b\"} 1\n",
            2,
            false,
        ),
    ];
    for (format_name, body, line_no, also_default) in cases {
        assert_conflict(format_name, body, line_no, also_default);
    }
}

#[test]
fn repeated_host_value_is_allotment_100_on_pro() {
    let cases = [
        ("dogstatsd", "m:1|c|#host:a,host:a\n"),
        ("dogstatsd", "m:1|c|#host:a, host:a\n"),
        (
            "jsonl",
            "{\"metric\":\"m\",\"type\":\"count\",\"tags\":[\"host:a\",\"host:a\"]}\n",
        ),
        (
            "prometheus",
            "# TYPE m counter\nm{host=\"a\",host=\"a\"} 1\n",
        ),
    ];
    for (format_name, body) in cases {
        let path = TempPath::text(body);
        let output = run(&[
            "count".to_string(),
            "--format".to_string(),
            format_name.to_string(),
            "--plan".to_string(),
            "pro".to_string(),
            path.path().to_string(),
        ]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{format_name} {body:?}\n{}",
            failure(&output)
        );
        let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
        assert!(
            stdout.contains("allotment: 100\n"),
            "{format_name} {body:?}\n{stdout}"
        );
        assert!(
            stdout.contains("total: 1\n"),
            "{format_name} {body:?}\n{stdout}"
        );
    }
}

#[test]
fn other_duplicate_keys_stay_allowed() {
    let cases = [
        ("dogstatsd", "m:1|c|#env:a,env:b,host:a\n"),
        (
            "jsonl",
            "{\"metric\":\"m\",\"type\":\"count\",\"tags\":[\"env:a\",\"env:b\",\"host:a\"]}\n",
        ),
        (
            "prometheus",
            "# TYPE m counter\nm{env=\"a\",env=\"b\",host=\"a\"} 1\n",
        ),
    ];
    for (format_name, body) in cases {
        let path = TempPath::text(body);
        let output = run(&[
            "count".to_string(),
            "--format".to_string(),
            format_name.to_string(),
            "--plan".to_string(),
            "pro".to_string(),
            path.path().to_string(),
        ]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{format_name}\n{}",
            failure(&output)
        );
        let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
        assert!(stdout.contains("allotment: 100\n"), "{stdout}");
        assert!(stdout.contains("total: 1\n"), "{stdout}");
    }
}

/// Keys are not lowercased, so `HOST` is not the host tag.
#[test]
fn host_key_is_case_sensitive() {
    let none = TempPath::text("m:1|c|#HOST:a,HOST:b\n");
    let output = run(&[
        "count".to_string(),
        "--plan".to_string(),
        "pro".to_string(),
        none.path().to_string(),
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.contains("allotment: 0\n"), "{stdout}");

    let mixed = [
        ("dogstatsd", "m:1|c|#HOST:a,host:b\n"),
        (
            "jsonl",
            "{\"metric\":\"m\",\"type\":\"count\",\"tags\":[\"HOST:a\",\"host:b\"]}\n",
        ),
        (
            "prometheus",
            "# TYPE m counter\nm{HOST=\"a\",host=\"b\"} 1\n",
        ),
    ];
    for (format_name, body) in mixed {
        let path = TempPath::text(body);
        let output = run(&[
            "count".to_string(),
            "--format".to_string(),
            format_name.to_string(),
            "--plan".to_string(),
            "pro".to_string(),
            path.path().to_string(),
        ]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{format_name}\n{}",
            failure(&output)
        );
        let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
        assert!(
            stdout.contains("allotment: 100\n"),
            "{format_name}\n{stdout}"
        );
    }
}

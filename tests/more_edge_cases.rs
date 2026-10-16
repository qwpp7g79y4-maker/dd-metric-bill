//! Edge cases the format allows that `edge_cases.rs` does not cover yet:
//! a lone CR, a very long line, NUL bytes, Unicode whitespace and
//! lookalike letters, and numbers such as `1e309`, `-0`, and huge integers.

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
        let path = std::env::temp_dir().join(format!("dd-metric-bill-test-more-{nanos}-{n}.txt"));
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

fn stdout_of(args: &[&str]) -> String {
    let output = run(args);
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    String::from_utf8(output.stdout).expect("stdout is utf-8")
}

/// `str::lines()` only splits on `\n` (stripping a preceding `\r`). A lone
/// `\r` not followed by `\n` is not a line break: it stays embedded in the
/// line, so `name:value|c\rname:value|c` is one shape-broken line, not two
/// valid ones. This is current, documented behavior, not a crash.
#[test]
fn lone_cr_stays_embedded_and_breaks_the_shape() {
    let dog = TempPath::text("a:1|c\rb:1|c\n");
    let output = run(&["count", "--format", "dogstatsd", dog.path()]);
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert_eq!(
        stderr,
        "invalid dogstatsd line 1: type is missing or unknown\n"
    );

    let prom = TempPath::text("# TYPE a counter\ra{host=\"x\"} 1\n");
    let output = run(&["count", "--format", "prometheus", prom.path()]);
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert_eq!(stderr, "invalid prometheus line 1: shape is wrong\n");
}

/// CRLF still works when the `\r` is immediately before the `\n` (already
/// covered elsewhere); a single very long tag value does not slow the
/// parser down or lose any of its length.
#[test]
fn very_long_tag_value_parses_completely() {
    let long_value = "v".repeat(2_000_000);
    let path = TempPath::text(&format!("a:1|c|#k:{long_value}\n"));
    let start = std::time::Instant::now();
    let stdout = stdout_of(&["count", path.path()]);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "took {:?}",
        start.elapsed()
    );
    assert!(stdout.contains("total: 1\n"), "{stdout}");
    assert!(
        stdout.contains("a type: count combinations: 1 count: 1 rule: D3"),
        "{stdout}"
    );
}

/// A NUL byte is valid UTF-8 and stays part of a metric name. Text output
/// escapes it like any other control character; JSON keeps the raw byte
/// (serde_json escapes only what JSON requires, and NUL is one of those).
#[test]
fn nul_byte_in_name_is_escaped_in_text_and_raw_in_json() {
    let path = TempPath::text("a\0b:1|c\n");
    let text = stdout_of(&["count", path.path()]);
    assert!(text.contains("a\\x00b type: count"), "{text:?}");
    assert!(!text.contains('\0'), "{text:?}");

    let json = stdout_of(&["count", "--json", path.path()]);
    let doc: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    assert_eq!(doc["metrics"][0]["name"], "a\u{0}b");
}

/// Unicode whitespace (NBSP here) is trimmed from the edges of a name the
/// same way ASCII space is; it is not trimmed from the inside of a value.
#[test]
fn unicode_whitespace_is_trimmed_like_ascii() {
    let nbsp = "\u{a0}";
    let path = TempPath::text(&format!("{nbsp}a{nbsp}:1|c|#k:x{nbsp}y\n"));
    let stdout = stdout_of(&["count", path.path()]);
    assert!(
        stdout.contains("a type: count combinations: 1 count: 1 rule: D3"),
        "{stdout}"
    );
    assert!(!stdout.contains("\u{a0}a"), "{stdout}");

    let json = stdout_of(&["count", "--json", path.path()]);
    let doc: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    assert_eq!(doc["tags"][0]["keys"][0]["key"], "k");
}

/// A Cyrillic "а" (U+0430) and a Latin "a" are different byte sequences:
/// no Unicode normalization folds them into the same metric.
#[test]
fn lookalike_letters_are_distinct_metrics() {
    let path = TempPath::text("a:1|c\n\u{430}:1|c\n");
    let stdout = stdout_of(&["count", path.path()]);
    assert!(stdout.contains("total: 2\n"), "{stdout}");
    assert_eq!(
        stdout
            .lines()
            .filter(|line| line.contains("type: count combinations: 1 count: 1"))
            .count(),
        2,
        "{stdout}"
    );
}

/// DogStatsD and Prometheus values are not restricted to the price rules:
/// `-0` is finite and is accepted like any other value.
#[test]
fn negative_zero_value_is_accepted() {
    let dog = TempPath::text("a:-0|c\n");
    let stdout = stdout_of(&["count", "--format", "dogstatsd", dog.path()]);
    assert!(stdout.contains("total: 1\n"), "{stdout}");

    let prom = TempPath::text("# TYPE a gauge\na -0\n");
    let stdout = stdout_of(&["count", "--format", "prometheus", prom.path()]);
    assert!(stdout.contains("total: 1\n"), "{stdout}");
}

/// JSONL ignores the `value` field entirely, so `1e309`, `-0`, and an
/// integer far outside `u64` range do not change whether the line parses.
#[test]
fn jsonl_value_field_is_ignored_whatever_number_it_holds() {
    for value in [
        "1e309",
        "-0",
        "99999999999999999999999999999",
        "-99999999999999999999999999999",
    ] {
        let path = TempPath::text(&format!(
            "{{\"metric\":\"a\",\"type\":\"count\",\"value\":{value}}}\n"
        ));
        let stdout = stdout_of(&["count", "--format", "jsonl", path.path()]);
        assert!(stdout.contains("total: 1\n"), "{value}\n{stdout}");
    }
}

fn price_rejected(ingested: &str, expected: &str) {
    let json = format!(
        r#"{{"currency":"TST","source":"s","date":"2026-01-01","ingested_per_100":{ingested}}}"#
    );
    let price = TempPath::text(&json);
    let sample = TempPath::text("a:1|c\n");
    let output = run(&["count", "--prices", price.path(), sample.path()]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{ingested}\n{}",
        failure(&output)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert!(
        stderr.contains(expected),
        "{ingested}: expected {expected:?} in {stderr:?}"
    );
}

fn price_accepted(ingested: &str) {
    let json = format!(
        r#"{{"currency":"TST","source":"s","date":"2026-01-01","ingested_per_100":{ingested}}}"#
    );
    let price = TempPath::text(&json);
    let sample = TempPath::text("a:1|c\n");
    let output = run(&["count", "--prices", price.path(), sample.path()]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{ingested}\n{}",
        failure(&output)
    );
}

/// Scientific notation in a price: `1e309` and `1e-13` are both too large
/// (too big a numerator, or more than 12 digits after the point); `1.5e2`
/// is the plain price `150`.
#[test]
fn price_scientific_notation() {
    price_rejected("1e309", "field is too large");
    price_rejected("1E400", "field is too large");
    price_rejected("1e-13", "field is too large");
    price_rejected("9e37", "field is too large");
    price_accepted("1.5e2");
    price_accepted("1e-12");
    price_accepted("100e-2");
}

/// `-0` as a plain JSON integer is accepted (price `0`): `serde_json`
/// itself normalizes the integer literal `-0` to `0` before this tool ever
/// sees a sign to reject, unlike `-0.0` and `-0e0`, which keep their sign
/// and are rejected as negative. The computed cost is `0` either way; this
/// test documents the inconsistency between the three spellings rather
/// than asserting it is the ideal behavior.
#[test]
fn negative_zero_price_forms_are_not_rejected_the_same_way() {
    price_accepted("-0");
    price_rejected("-0.0", "field is negative");
    price_rejected("-0e0", "field is negative");
}

//! Unusual input. Temporary files, no fixtures.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static SERIAL: AtomicU64 = AtomicU64::new(0);

struct TempPath(PathBuf);

impl TempPath {
    fn bytes(contents: &[u8]) -> Self {
        let n = SERIAL.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("dd-metric-bill-test-edge-{nanos}-{n}.txt"));
        std::fs::write(&path, contents).expect("write sample");
        Self(path)
    }

    fn text(contents: &str) -> Self {
        Self::bytes(contents.as_bytes())
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

fn run(args: &[&str]) -> Output {
    binary().args(args).output().expect("binary start")
}

const LINE_A: &str = "a type: count combinations: 1 count: 1 rule: D3";

/// (a) A BOM at the start of the file is not part of the first metric.
/// Without that skip, `\u{feff}a` is a different metric from `a`, also in `diff`.
/// JSONL and Prometheus then exit 2.
#[test]
fn bom_at_the_start_is_skipped() {
    let bom = "\u{feff}";
    let dog = TempPath::text(&format!("{bom}a:1|c\n"));
    let jsonl = TempPath::text(&format!("{bom}{{\"metric\":\"a\",\"type\":\"count\"}}\n"));
    let prom = TempPath::text(&format!("{bom}# TYPE a counter\na 1\n"));
    for (format_name, path) in [
        ("dogstatsd", dog.path()),
        ("jsonl", jsonl.path()),
        ("prometheus", prom.path()),
    ] {
        let output = run(&["count", "--format", format_name, path]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{format_name}\n{}",
            failure(&output)
        );
        let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
        assert!(
            stdout.lines().any(|line| line == LINE_A),
            "{format_name} counts the BOM:\n{stdout:?}"
        );
        assert!(
            !stdout.contains('\u{feff}'),
            "{format_name} shows the BOM:\n{stdout:?}"
        );
    }

    let without = TempPath::text("a:1|c\n");
    let diff = run(&["diff", dog.path(), without.path()]);
    assert_eq!(diff.status.code(), Some(0), "{}", failure(&diff));
    let stdout = String::from_utf8(diff.stdout).expect("stdout is utf-8");
    assert!(stdout.contains("delta: 0\n"), "{stdout:?}");
    assert!(
        !stdout.contains('\u{feff}'),
        "diff shows the BOM:\n{stdout:?}"
    );
    assert!(
        !stdout
            .lines()
            .any(|line| line == "new:" || line == "removed:"),
        "BOM file and file without a BOM are not the same metric:\n{stdout:?}"
    );

    // Only the start of the file. A BOM on a later line stays part of that name.
    let later = TempPath::text(&format!("a:1|c\n{bom}b:1|c\n"));
    let output = run(&["count", later.path()]);
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.lines().any(|line| line == LINE_A), "{stdout:?}");
    assert!(
        stdout.contains('\u{feff}'),
        "a BOM in the middle of the file should stay part of the name:\n{stdout:?}"
    );
}

const OPENMETRICS: &str = "https://docs.datadoghq.com/integrations/openmetrics/";

/// A `_total` sample of a counter family counts as `a.count` (B3).
/// `_created` does not count.
/// `# TYPE a_total counter` with a sample named `a_total` stays 1, under that name.
#[test]
fn counter_total_counts_and_created_does_not() {
    let input = "\
# TYPE a counter
a_total{host=\"hh-one\"} 1
a_total{host=\"hh-two\"} 2
a_created{host=\"hh-one\"} 1.7e9
a_created{host=\"hh-two\"} 1.7e9
";
    let path = TempPath::text(input);
    let cases: [&[&str]; 2] = [
        &["count", "--format", "prometheus", "--explain", path.path()],
        &[
            "count",
            "--format",
            "prometheus",
            "--untyped-as",
            "gauge",
            "--explain",
            path.path(),
        ],
    ];
    for args in cases {
        let output = run(args);
        assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
        let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
        assert!(stdout.contains("total: 2\n"), "{stdout}");
        assert!(
            stdout.contains("a.count type: count combinations: 2 count: 2 rule: D12"),
            "{stdout}"
        );
        assert!(stdout.contains(OPENMETRICS), "{stdout}");
        assert!(stdout.contains("  D12: "), "{stdout}");
        assert!(
            !stdout.contains("a_created"),
            "created sample was counted:\n{stdout}"
        );
        assert!(
            !stdout.contains("a_total"),
            "`a_total` was kept as its own name:\n{stdout}"
        );
        assert!(
            !stdout.contains("hh-one") && !stdout.contains("hh-two"),
            "{stdout}"
        );
    }

    let named = TempPath::text("# TYPE a_total counter\na_total{host=\"hh-three\"} 1\n");
    let output = run(&["count", "--format", "prometheus", named.path()]);
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(
        stdout.contains("a_total type: count combinations: 1 count: 1 rule: D3"),
        "{stdout}"
    );
    assert!(!stdout.contains("a.count"), "{stdout}");
    assert!(!stdout.contains("hh-three"), "{stdout}");
}

/// A label list may end with a comma. That line still counts.
#[test]
fn prometheus_label_comma_at_the_end_counts() {
    let path = TempPath::text(
        "\
# TYPE a gauge
# TYPE b gauge
# TYPE c counter
a{host=\"hh-one\",} 1
b{host=\"hh-two\",env=\"prod\",} 2
c_total{host=\"hh-three\",} 3
",
    );
    let output = run(&["count", "--format", "prometheus", path.path()]);
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.contains("total: 3\n"), "{stdout}");
    assert!(
        stdout.contains("a type: gauge combinations: 1 count: 1 rule: D3"),
        "{stdout}"
    );
    assert!(
        stdout.contains("b type: gauge combinations: 1 count: 1 rule: D3"),
        "{stdout}"
    );
    assert!(
        stdout.contains("c.count type: count combinations: 1 count: 1 rule: D12"),
        "{stdout}"
    );
    assert!(
        !stdout.contains("hh-one") && !stdout.contains("hh-two") && !stdout.contains("hh-three"),
        "{stdout}"
    );
}

/// (d) Control characters in a metric name or tag key are visibly
/// escaped in text. JSON keeps the raw string (the JSON encoder writes `\u001b`).
/// A tag value appears nowhere in the output.
#[test]
fn control_characters_are_visible_in_text_json_stays_raw() {
    let value = "tag_value-canary";
    let dog = format!("web\u{1b}[31m:1|c|#h\u{1b}ost:{value}\n");
    let path = TempPath::text(&dog);
    let text = run(&["count", path.path()]);
    assert_eq!(text.status.code(), Some(0), "{}", failure(&text));
    let stdout = String::from_utf8(text.stdout).expect("stdout is utf-8");
    assert!(
        !stdout.contains('\u{1b}'),
        "text contains a raw ESC:\n{stdout:?}"
    );
    assert!(stdout.contains("web\\x1b[31m type: count"), "{stdout:?}");
    assert!(stdout.contains("tag: h\\x1bost values:"), "{stdout:?}");
    assert!(!stdout.contains(value), "{stdout:?}");

    let json = run(&["count", "--json", path.path()]);
    assert_eq!(json.status.code(), Some(0), "{}", failure(&json));
    let raw_json = String::from_utf8(json.stdout).expect("json is utf-8");
    assert!(
        !raw_json.contains('\u{1b}'),
        "json contains a raw ESC:\n{raw_json:?}"
    );
    assert!(
        raw_json.contains("\"name\": \"web\\u001b[31m\""),
        "{raw_json}"
    );
    assert!(raw_json.contains("\"key\": \"h\\u001bost\""), "{raw_json}");
    assert!(
        !raw_json.contains("\\x1b"),
        "json uses the text escape:\n{raw_json}"
    );
    assert!(!raw_json.contains(value), "{raw_json}");

    let jsonl = TempPath::text(&format!(
        "{{\"metric\":\"web\\u001b[31m\",\"type\":\"count\",\"tags\":[\"h\\u001bost:{value}\"]}}\n"
    ));
    let out_jsonl = run(&["count", "--format", "jsonl", jsonl.path()]);
    assert_eq!(out_jsonl.status.code(), Some(0), "{}", failure(&out_jsonl));
    let stdout = String::from_utf8(out_jsonl.stdout).expect("stdout is utf-8");
    assert!(!stdout.contains('\u{1b}'), "{stdout:?}");
    assert!(stdout.contains("web\\x1b[31m type: count"), "{stdout:?}");
    assert!(stdout.contains("tag: h\\x1bost values:"), "{stdout:?}");
    assert!(!stdout.contains(value), "{stdout:?}");

    let old = TempPath::text("");
    let diff = run(&["diff", old.path(), path.path()]);
    assert_eq!(diff.status.code(), Some(0), "{}", failure(&diff));
    let stdout = String::from_utf8(diff.stdout).expect("stdout is utf-8");
    assert!(
        !stdout.contains('\u{1b}'),
        "diff contains a raw ESC:\n{stdout:?}"
    );
    assert!(stdout.contains("web\\x1b[31m type: count"), "{stdout:?}");
    assert!(stdout.contains("tag: h\\x1bost "), "{stdout:?}");
    assert!(!stdout.contains(value), "{stdout:?}");
}

fn stdout_of(args: &[&str]) -> String {
    let output = run(args);
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    String::from_utf8(output.stdout).expect("stdout is utf-8")
}

fn same_count(format_name: &str, lines: &[&str]) {
    let base_path = TempPath::text(lines[0]);
    let base_out = stdout_of(&["count", "--format", format_name, base_path.path()]);
    assert!(base_out.contains("total: 1\n"), "{format_name}\n{base_out}");
    for line in &lines[1..] {
        let path = TempPath::text(line);
        let stdout = stdout_of(&["count", "--format", format_name, path.path()]);
        assert_eq!(stdout, base_out, "{format_name} input {line:?}");
    }
}

#[test]
fn empty_file_and_whitespace_count_zero() {
    for format_name in ["dogstatsd", "jsonl", "prometheus"] {
        let mut base_out = None;
        for contents in ["", " \n\t\n", "\r\n"] {
            let path = TempPath::text(contents);
            let stdout = stdout_of(&["count", "--format", format_name, path.path()]);
            assert!(stdout.contains("total: 0\n"), "{format_name}\n{stdout}");
            if let Some(first) = &base_out {
                assert_eq!(&stdout, first, "{format_name}");
            } else {
                base_out = Some(stdout);
            }
        }
    }
}

#[test]
fn crlf_counts_the_same_as_lf() {
    let cases = [
        ("dogstatsd", "a:1|c\n", "a:1|c\r\n"),
        (
            "jsonl",
            "{\"metric\":\"a\",\"type\":\"count\"}\n",
            "{\"metric\":\"a\",\"type\":\"count\"}\r\n",
        ),
        (
            "prometheus",
            "# TYPE a counter\na 1\n",
            "# TYPE a counter\r\na 1\r\n",
        ),
    ];
    for (format_name, lf, crlf) in cases {
        let left = TempPath::text(lf);
        let right = TempPath::text(crlf);
        let a = stdout_of(&["count", "--format", format_name, left.path()]);
        let b = stdout_of(&["count", "--format", format_name, right.path()]);
        assert_eq!(a, b, "{format_name}");
        assert!(a.contains("total: 1\n"), "{format_name}\n{a}");
    }
}

#[test]
fn invalid_utf8_and_utf16_give_exit_2() {
    let marker = "CANARY-MARKER";
    let mut broken = marker.as_bytes().to_vec();
    broken.push(0xff);
    broken.extend_from_slice(b":1|c\n");
    let mut utf16 = vec![0xFF, 0xFE];
    for unit in format!("{marker}:1|c\n").encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    for format_name in ["dogstatsd", "jsonl", "prometheus"] {
        for contents in [&broken, &utf16] {
            let path = TempPath::bytes(contents);
            let output = run(&["count", "--format", format_name, path.path()]);
            assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
            assert!(output.stdout.is_empty(), "{}", failure(&output));
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("cannot read sample"), "{stderr}");
            assert!(!stderr.contains(marker), "{stderr}");
        }
    }
}

#[test]
fn huge_finite_value_counts_nan_and_inf_exit_2() {
    same_count("dogstatsd", &["a:1|c\n", "a:1e308|c\n"]);
    for line in ["a:NaN|c\n", "a:+Inf|c\n"] {
        let path = TempPath::text(line);
        let output = run(&["count", "--format", "dogstatsd", path.path()]);
        assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
        assert!(output.stdout.is_empty(), "{}", failure(&output));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("line 1"), "{stderr}");
        assert!(!stderr.contains(line.trim()), "{stderr}");
    }
    same_count(
        "prometheus",
        &[
            "# TYPE a gauge\na 1\n",
            "# TYPE a gauge\na 1e308\n",
            "# TYPE a gauge\na NaN\n",
            "# TYPE a gauge\na +Inf\n",
        ],
    );
    same_count(
        "jsonl",
        &[
            "{\"metric\":\"a\",\"type\":\"count\"}\n",
            "{\"metric\":\"a\",\"type\":\"count\",\"value\":1}\n",
            "{\"metric\":\"a\",\"type\":\"count\",\"value\":1e308}\n",
        ],
    );
    // NaN is not JSON. The line is rejected, so there is no count.
    let path = TempPath::text("{\"metric\":\"canary-nan\",\"type\":\"count\",\"value\":NaN}\n");
    let output = run(&["count", "--format", "jsonl", path.path()]);
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("line 1"), "{stderr}");
    assert!(!stderr.contains("canary-nan"), "{stderr}");
}

#[test]
fn jsonl_without_tags_counts_without_metric_or_type_exit_2() {
    let with = TempPath::text("{\"metric\":\"a\",\"type\":\"count\"}\n");
    let stdout = stdout_of(&["count", "--format", "jsonl", with.path()]);
    assert!(
        stdout.contains("a type: count combinations: 1 count: 1 rule: D3"),
        "{stdout}"
    );

    let without_metric = TempPath::text("{\"type\":\"count\",\"extra\":\"canary-line\"}\n");
    let output = run(&["count", "--format", "jsonl", without_metric.path()]);
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("line 1"), "{stderr}");
    assert!(!stderr.contains("canary-line"), "{stderr}");

    let without_type =
        TempPath::text("{\"metric\":\"a\",\"type\":\"count\"}\n{\"metric\":\"canary-name\"}\n");
    let output = run(&["count", "--format", "jsonl", without_type.path()]);
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("line 2"), "{stderr}");
    assert!(!stderr.contains("canary-name"), "{stderr}");
}

/// UTF-16 and UTF-32 text without a BOM is valid UTF-8 full of NUL bytes.
/// Whatever the parser makes of it, a tag value or host name never shows up,
/// not even with the NUL bytes or their escapes taken out.
#[test]
fn utf16_and_utf32_never_show_tag_values() {
    let samples = [
        ("dogstatsd", "req:1|c|#user:CANARYUSER,host:CANARYHOST\n"),
        (
            "jsonl",
            "{\"metric\":\"req\",\"type\":\"count\",\"tags\":[\"user:CANARYUSER\",\"host:CANARYHOST\"]}\n",
        ),
        (
            "prometheus",
            "# TYPE req counter\nreq{user=\"CANARYUSER\",host=\"CANARYHOST\"} 1\n",
        ),
    ];
    for (format_name, text) in samples {
        let units: Vec<u16> = text.encode_utf16().collect();
        let encodings: [(&str, Vec<u8>); 4] = [
            (
                "utf-16le",
                units.iter().flat_map(|u| u.to_le_bytes()).collect(),
            ),
            (
                "utf-16be",
                units.iter().flat_map(|u| u.to_be_bytes()).collect(),
            ),
            (
                "utf-32le",
                text.chars()
                    .flat_map(|c| (c as u32).to_le_bytes())
                    .collect(),
            ),
            (
                "utf-32be",
                text.chars()
                    .flat_map(|c| (c as u32).to_be_bytes())
                    .collect(),
            ),
        ];
        for (encoding, contents) in encodings {
            let path = TempPath::bytes(&contents);
            for extra in [&["--explain"][..], &["--json"][..]] {
                let mut args = vec!["count", "--format", format_name];
                args.extend_from_slice(extra);
                args.push(path.path());
                let output = run(&args);
                let all = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                )
                .replace(path.path(), "");
                let squeezed: String = all.chars().filter(|c| c.is_ascii_uppercase()).collect();
                for canary in ["CANARYUSER", "CANARYHOST"] {
                    assert!(
                        !squeezed.contains(canary),
                        "{format_name} {encoding} {extra:?} shows {canary}: {all:?}"
                    );
                }
            }
        }
    }
}

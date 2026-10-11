use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Parallel tests can read the same clock value (Windows has a coarse clock), so a counter keeps names unique.
static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

const ESTIMATE: &str =
    "estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)";

const SENTINEL: &str = "zz-tag_value-9f3e2a7c";

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

const USER_ID_TAG_VALUES: &[&str] = &["u1", "u2", "u3", "prod", "pay", "cart"];

const DIFF_TAG_TEXT: &[&str] = &["host:a", "host:b", "region:eu", "region:us", "env:prod"];

const DIFF_TAGTOKENS: &[&str] = &["prod", "eu", "us", "a", "b"];

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

fn fixture(name: &str) -> String {
    format!("tests/fixtures/{name}")
}

fn output(args: &[impl AsRef<OsStr>]) -> (u8, String, String) {
    let output = binary().args(args).output().expect("binary start");
    let code = output.status.code().unwrap_or(255);
    if code != 0 && code != 1 {
        panic!("unexpected exit {code}: {}", failure(&output));
    }
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    let code = u8::try_from(code).expect("exit code fits in u8");
    (code, stdout, stderr)
}

fn assert_snapshot(name: &str, args: &[impl AsRef<OsStr>]) -> String {
    let (code, got, stderr) = output(args);
    assert_eq!(code, 0, "stderr={stderr}\nstdout={got}");
    assert!(stderr.is_empty(), "stderr is not empty:\n{stderr}");
    assert_eq!(got.matches(ESTIMATE).count(), 1, "{got}");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(name);
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read snapshot {}: {err}", path.display()));
    assert_eq!(got, expected, "snapshot {name}");
    valid_via_json_tool(&got);
    got
}

fn valid_via_json_tool(text: &str) {
    let mut child = Command::new("python3")
        .args(["-m", "json.tool"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("python3 start");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(text.as_bytes()).expect("write json");
    }
    let output = child.wait_with_output().expect("python3 finished");
    assert!(
        output.status.success(),
        "python3 -m json.tool fails: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn python_check(text: &str, script: &str) {
    let mut child = Command::new("python3")
        .args(["-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("python3 start");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(text.as_bytes()).expect("write json");
    }
    let output = child.wait_with_output().expect("python3 finished");
    assert!(
        output.status.success(),
        "json-check fails: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

fn has_token(text: &str, token: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word == token)
}

fn assert_not_text(got: &str, values: &[&str]) {
    for value in values {
        assert!(
            !got.contains(value),
            "output shows tag value {value}:\n{got}"
        );
    }
}

fn assert_not_tokens(got: &str, values: &[&str]) {
    for value in values {
        assert!(
            !has_token(got, value),
            "output shows tag value {value}:\n{got}"
        );
    }
}

fn assert_nowhere(stdout: &str, stderr: &str, values: &[&str]) {
    assert_not_text(stdout, values);
    assert_not_text(stderr, values);
}

struct TempSample(PathBuf);

impl TempSample {
    fn new(contents: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let n = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("dd-metric-bill-test-json-{nanos}-{n}.txt"));
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
fn b1_count_json() {
    let got = assert_snapshot(
        "b1-count.json",
        &["count", "--json", &fixture("b1-count.dogstatsd")],
    );
    python_check(
        &got,
        r#"
import json, sys
doc = json.load(sys.stdin)
assert doc["schema_version"] == 1
assert doc["tool"] == "dd-metric-bill"
assert doc["total"] == 4
assert doc["allotment"] is None
assert doc["unbounded"] == []
assert "ingested" not in doc and "indexed" not in doc and "costs" not in doc
assert set(doc) == {
    "schema_version", "tool", "estimate", "source", "inputs",
    "total", "metrics", "tags", "unbounded", "allotment",
}
metric = doc["metrics"][0]
assert metric["name"] == "request.Latency"
assert metric["type"] == "count"
assert metric["combinations"] == 4
assert metric["count"] == 4
assert metric["rule"] == "D3"
assert "ingested" not in metric and "indexed" not in metric
assert [tag["key"] for tag in doc["tags"][0]["keys"]] == ["endpoint", "host", "status"]
assert doc["tags"][0]["keys"][0]["values"] == 2
assert doc["tags"][0]["keys"][0]["added"] == 1
"#,
    );
    assert_not_text(&got, B1_TAG_VALUES);
}

#[test]
fn b1_count_json_allow_and_price() {
    let got = assert_snapshot(
        "b1-count-allow-price.json",
        &[
            "count",
            "--json",
            "--allow-tags",
            "endpoint,status",
            "--plan",
            "pro",
            "--hosts",
            "3",
            "--prices",
            "prices/datadog-example.json",
            &fixture("b1-count.dogstatsd"),
        ],
    );
    python_check(
        &got,
        r#"
import json, sys
doc = json.load(sys.stdin)
assert doc["schema_version"] == 1
assert doc["ingested"] == 4
assert doc["indexed"] == 3
assert doc["metrics"][0]["ingested"] == 4
assert doc["metrics"][0]["indexed"] == 3
assert doc["allotment"]["plan"] == "pro"
assert doc["allotment"]["hosts"] == 3
assert doc["allotment"]["allotment"] == 300
assert doc["allotment"]["indexed"] == 3
assert doc["allotment"]["indexed_surplus"] == 0
assert doc["allotment"]["ingested"] == 4
assert doc["allotment"]["ingested_surplus"] == 0
assert "usage" not in doc["allotment"] and "basis" not in doc["allotment"]
assert doc["costs"]["currency"] == "USD"
assert doc["costs"]["date"] == "2026-10-07"
assert doc["costs"]["lines"][0]["kind"] == "ingested"
assert doc["costs"]["lines"][0]["above"] == 0
assert doc["costs"]["lines"][0]["amount"] == "0"
assert "indexed" not in {line["kind"] for line in doc["costs"]["lines"]}
"#,
    );
    assert_not_text(&got, B1_TAG_VALUES);
}

#[test]
fn diff_json() {
    let got = assert_snapshot(
        "diff-tag-and-metric.json",
        &[
            "diff",
            "--json",
            &fixture("diff-old.dogstatsd"),
            &fixture("diff-new.dogstatsd"),
        ],
    );
    python_check(
        &got,
        r#"
import json, sys
doc = json.load(sys.stdin)
assert doc["schema_version"] == 1
assert doc["total"] == {"old": 3, "new": 4, "delta": 1}
assert doc["allotment"] is None
assert doc["unbounded"] == []
assert "ingested" not in doc and "indexed" not in doc and "costs" not in doc
assert doc["inputs"]["old"].endswith("diff-old.dogstatsd")
assert doc["inputs"]["new"].endswith("diff-new.dogstatsd")
names = [(m["name"], m["old"], m["new"], m["delta"]) for m in doc["metrics"]]
assert names == [
    ("checkout", 0, 1, 1),
    ("page.views", 2, 3, 1),
    ("queue.depth", 1, 0, -1),
]
region = next(tag for tag in doc["tags"] if tag["name"] == "page.views")
assert region["keys"][0]["key"] == "region"
assert region["keys"][0]["delta"] == 1
assert region["keys"][0]["new_values"] == 2
"#,
    );
    assert_not_text(&got, DIFF_TAG_TEXT);
    assert_not_tokens(&got, DIFF_TAGTOKENS);
}

#[test]
fn json_explain_does_not_change_the_output() {
    let without = assert_snapshot(
        "b1-count.json",
        &["count", "--json", &fixture("b1-count.dogstatsd")],
    );
    let (code, with, stderr) = output(&[
        "count",
        "--json",
        "--explain",
        &fixture("b1-count.dogstatsd"),
    ]);
    assert_eq!(code, 0);
    assert!(stderr.is_empty(), "{stderr}");
    assert_eq!(with, without);
}

#[test]
fn two_samples_keep_inputs_and_total() {
    let (code, got, stderr) = output(&[
        "count",
        "--json",
        &fixture("b1-count.dogstatsd"),
        &fixture("b1-histogram.dogstatsd"),
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    valid_via_json_tool(&got);
    python_check(
        &got,
        r#"
import json, sys
doc = json.load(sys.stdin)
assert doc["schema_version"] == 1
assert doc["total"] == 24
assert len(doc["inputs"]) == 2
assert doc["inputs"][0].endswith("b1-count.dogstatsd")
assert doc["inputs"][1].endswith("b1-histogram.dogstatsd")
assert [(m["type"], m["count"]) for m in doc["metrics"]] == [("count", 4), ("histogram", 20)]
assert "ingested" not in doc and "costs" not in doc
"#,
    );
    assert_not_text(&got, B1_TAG_VALUES);
}

#[test]
fn unbounded_tag_in_json_shows_the_reason_without_value() {
    let uuid = "550e8400-e29b-41d4-a716-446655440000";
    let path = TempSample::new(&format!("m:1|c|#env:prod,request_id:{uuid}\n"));
    let (code, got, stderr) = output(&["count", "--json", path.as_str()]);
    assert_eq!(code, 0, "{stderr}");
    valid_via_json_tool(&got);
    python_check(
        &got,
        r#"
import json, sys
doc = json.load(sys.stdin)
assert doc["unbounded"][0]["keys"][0]["key"] == "request_id"
assert doc["unbounded"][0]["keys"][0]["reasons"] == ["pattern uuid"]
assert all(tag["key"] != "env" for tag in doc["unbounded"][0]["keys"])
"#,
    );
    assert_nowhere(&got, &stderr, &[uuid, "prod"]);
}

#[test]
fn tag_values_appear_nowhere_without_show_values() {
    let silent = &[][..];
    let cases: &[(&[&str], &[&str], &[&str])] = &[
        (
            &["count", "tests/fixtures/b1-count.dogstatsd"],
            B1_TAG_VALUES,
            silent,
        ),
        (
            &["count", "--json", "tests/fixtures/b1-count.dogstatsd"],
            B1_TAG_VALUES,
            silent,
        ),
        (
            &["count", "tests/fixtures/mixed.dogstatsd"],
            MIXED_TAG_VALUES,
            silent,
        ),
        (
            &["count", "--json", "tests/fixtures/mixed.dogstatsd"],
            MIXED_TAG_VALUES,
            silent,
        ),
        (
            &["count", "tests/fixtures/user-id.dogstatsd"],
            USER_ID_TAG_VALUES,
            silent,
        ),
        (
            &["count", "--json", "tests/fixtures/user-id.dogstatsd"],
            USER_ID_TAG_VALUES,
            silent,
        ),
        (
            &[
                "diff",
                "tests/fixtures/diff-old.dogstatsd",
                "tests/fixtures/diff-new.dogstatsd",
            ],
            DIFF_TAG_TEXT,
            DIFF_TAGTOKENS,
        ),
        (
            &[
                "diff",
                "--json",
                "tests/fixtures/diff-old.dogstatsd",
                "tests/fixtures/diff-new.dogstatsd",
            ],
            DIFF_TAG_TEXT,
            DIFF_TAGTOKENS,
        ),
    ];
    for (args, text, tokens) in cases {
        let (code, stdout, stderr) = output(args);
        assert_eq!(code, 0, "{stderr}\n{stdout}");
        assert_nowhere(&stdout, &stderr, text);
        assert_not_tokens(&stdout, tokens);
        assert_not_tokens(&stderr, tokens);
        if args.contains(&"--json") {
            valid_via_json_tool(&stdout);
        }
    }

    let sample = format!("checkout:1|c|#env:{SENTINEL},user_id:{SENTINEL}\n");
    let path = TempSample::new(&sample);
    for json in [false, true] {
        let mut args = vec!["count"];
        if json {
            args.push("--json");
        }
        args.push(path.as_str());
        let (code, stdout, stderr) = output(&args);
        assert_eq!(code, 0, "{stderr}\n{stdout}");
        assert_nowhere(&stdout, &stderr, &[SENTINEL]);
        if json {
            valid_via_json_tool(&stdout);
            assert!(stdout.contains("\"key\": \"user_id\""), "{stdout}");
            assert!(stdout.contains("\"key\": \"env\""), "{stdout}");
        }
    }

    let old = TempSample::new("page.views:1|c|#host:web\n");
    let new = TempSample::new(&format!("page.views:1|c|#host:web,env:{SENTINEL}\n"));
    for json in [false, true] {
        let mut args = vec!["diff"];
        if json {
            args.push("--json");
        }
        args.push(old.as_str());
        args.push(new.as_str());
        let (code, stdout, stderr) = output(&args);
        assert_eq!(code, 0, "{stderr}\n{stdout}");
        assert_nowhere(&stdout, &stderr, &[SENTINEL]);
    }
}

#[test]
fn json_budget_exceeded_stays_exit_1() {
    let (code, stdout, stderr) = output(&[
        "count",
        "--json",
        "--budget",
        "3",
        &fixture("b1-count.dogstatsd"),
    ]);
    assert_eq!(code, 1, "{stderr}\n{stdout}");
    valid_via_json_tool(&stdout);
    assert!(stdout.contains("\"total\": 4"), "{stdout}");
    assert!(stderr.contains("budget exceeded: 4 > 3"), "{stderr}");
    assert_nowhere(&stdout, &stderr, B1_TAG_VALUES);
}

#[test]
fn cargo_lock_has_no_network_crates() {
    let lock = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock"))
        .expect("read Cargo.lock");
    for name in ["reqwest", "hyper", "ureq", "tokio"] {
        let package_name = format!("name = \"{name}\"");
        let dep = format!("\"{name}\"");
        let found = lock.lines().any(|line| {
            let line = line.trim();
            line == package_name || line == dep || line == format!("{dep},")
        });
        assert!(!found, "Cargo.lock names network crate {name}");
    }
}

#[test]
fn src_does_not_use_std_net() {
    let mut paths = Vec::new();
    collect_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut paths,
    );
    assert!(!paths.is_empty(), "src is empty");
    for path in paths {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        assert!(
            !text.contains("std::net"),
            "{} names std::net",
            path.display()
        );
    }
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in
        std::fs::read_dir(dir).unwrap_or_else(|err| panic!("read {}: {err}", dir.display()))
    {
        let entry = entry.expect("src item");
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

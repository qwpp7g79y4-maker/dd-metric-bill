use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dd_metric_bill::cost::{estimate_cost, parse_prices, CostKind};
use dd_metric_bill::count::CUSTOM_METRICS_SOURCE;

const ESTIMATE: &str =
    "estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)";

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
}

fn manifest(relative: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(relative)
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

fn contains_price_value(text: &str) -> Option<&str> {
    const TOKENS: &[&str] = &["0.10", "0,10", "$", "USD", "EUR", "€"];
    TOKENS.iter().copied().find(|token| text.contains(token))
}

/// Parallel tests can read the same clock value (Windows has a coarse clock), so a counter keeps names unique.
static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct TempFile(PathBuf);

impl TempFile {
    fn new(contents: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let n = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("dd-metric-bill-test-price-{nanos}-{n}.json"));
        std::fs::write(&path, contents).expect("write price file");
        Self(path)
    }

    fn as_str(&self) -> &str {
        self.0.to_str().expect("path is utf-8")
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn example_has_only_the_b1_price_for_ingested() {
    let path = manifest("prices/datadog-example.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"));
    assert!(text.contains("\"ingested_per_100\": 0.10"), "{text}");
    assert!(!text.contains("indexed"), "{text}");
    assert!(text.contains(CUSTOM_METRICS_SOURCE), "{text}");
    assert!(text.contains("2026-10-07"), "{text}");
    assert!(text.contains("\"currency\": \"USD\""), "{text}");

    let prices = parse_prices(&text).expect("example is valid");
    assert_eq!(prices.currency, "USD");
    assert_eq!(prices.source, CUSTOM_METRICS_SOURCE);
    assert_eq!(prices.date, "2026-10-07");

    let fifty_above = estimate_cost(&prices, Some(100), Some(150), 3);
    assert_eq!(fifty_above.lines.len(), 1);
    assert_eq!(fifty_above.lines[0].kind, CostKind::Ingested);
    assert_eq!(fifty_above.lines[0].above, 50);
    assert_eq!(fifty_above.lines[0].amount, "0.05");

    let one_above = estimate_cost(&prices, Some(0), Some(1), 0);
    assert_eq!(one_above.lines[0].amount, "0.001");
}

#[test]
fn src_contains_no_price_values() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    visit(&src_dir, &mut found);
    assert!(found.is_empty(), "src contains a price value: {found:?}");
}

fn visit(path: &Path, found: &mut Vec<String>) {
    for entry in std::fs::read_dir(path).unwrap_or_else(|err| panic!("{}: {err}", path.display())) {
        let entry = entry.unwrap_or_else(|err| panic!("{err}"));
        let path = entry.path();
        if path.is_dir() {
            visit(&path, found);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        if let Some(token) = contains_price_value(&text) {
            found.push(format!("{} contains {token}", path.display()));
        }
    }
}

#[test]
fn without_prices_the_binary_shows_no_currency() {
    let sample = manifest("tests/fixtures/b1-count.dogstatsd");
    let output = binary()
        .args(["count", &sample])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert!(contains_price_value(&stdout).is_none(), "{stdout}");
    assert!(contains_price_value(&stderr).is_none(), "{stderr}");
    assert!(!stdout.contains("price:"), "{stdout}");
    assert!(!stdout.contains("cost "), "{stdout}");
    assert!(!stdout.contains("date:"), "{stdout}");
    assert_eq!(stdout.matches(ESTIMATE).count(), 1, "{stdout}");
}

#[test]
fn binary_shows_source_date_and_cost_only_above_the_allotment() {
    let sample = manifest("tests/fixtures/b1-count.dogstatsd");
    let prices = manifest("prices/datadog-example.json");

    let under = binary()
        .args([
            "count",
            "--plan",
            "pro",
            "--hosts",
            "1",
            "--allow-tags",
            "endpoint,status",
            "--prices",
            &prices,
            &sample,
        ])
        .output()
        .expect("binary start");
    assert_eq!(under.status.code(), Some(0), "{}", failure(&under));
    let under = String::from_utf8(under.stdout).expect("stdout is utf-8");
    assert!(under.contains("price: USD per 100\n"), "{under}");
    assert!(
        under.contains(&format!("source: {CUSTOM_METRICS_SOURCE}\n")),
        "{under}"
    );
    assert!(under.contains("date: 2026-10-07\n"), "{under}");
    assert!(under.contains("cost ingested: 0 USD\n"), "{under}");
    assert!(!under.contains("cost indexed"), "{under}");
    assert!(!under.contains("host:A"), "{under}");
    assert_eq!(under.matches(ESTIMATE).count(), 1, "{under}");

    let above = binary()
        .args([
            "count",
            "--plan",
            "pro",
            "--hosts",
            "0",
            "--allow-tags",
            "endpoint,status",
            "--prices",
            &prices,
            &sample,
        ])
        .output()
        .expect("binary start");
    assert_eq!(above.status.code(), Some(0), "{}", failure(&above));
    let above = String::from_utf8(above.stdout).expect("stdout is utf-8");
    assert!(above.contains("allotment: 0\n"), "{above}");
    assert!(above.contains("cost ingested: 0.004 USD\n"), "{above}");
    assert!(!above.contains("cost indexed"), "{above}");
    assert!(!above.contains("endpoint:X"), "{above}");

    let without_plan = binary()
        .args([
            "count",
            "--allow-tags",
            "endpoint,status",
            "--prices",
            &prices,
            &sample,
        ])
        .output()
        .expect("binary start");
    assert_eq!(
        without_plan.status.code(),
        Some(0),
        "{}",
        failure(&without_plan)
    );
    let without_plan = String::from_utf8(without_plan.stdout).expect("stdout is utf-8");
    assert!(
        without_plan.contains("price: USD per 100\n"),
        "{without_plan}"
    );
    assert!(
        without_plan.contains("date: 2026-10-07\n"),
        "{without_plan}"
    );
    assert!(!without_plan.contains("cost "), "{without_plan}");
}

#[test]
fn binary_computes_indexed_when_the_file_has_that_price() {
    let sample = manifest("tests/fixtures/b1-count.dogstatsd");
    let price = TempFile::new(
        r#"{"currency":"TST","source":"https://example.test/source","date":"2026-01-02","ingested_per_100":100,"indexed_per_100":50}"#,
    );
    let output = binary()
        .args([
            "count",
            "--plan",
            "pro",
            "--hosts",
            "0",
            "--allow-tags",
            "endpoint,status",
            "--prices",
            price.as_str(),
            &sample,
        ])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.contains("price: TST per 100\n"), "{stdout}");
    assert!(
        stdout.contains("source: https://example.test/source\n"),
        "{stdout}"
    );
    assert!(stdout.contains("date: 2026-01-02\n"), "{stdout}");
    assert!(stdout.contains("cost ingested: 4 TST\n"), "{stdout}");
    assert!(stdout.contains("cost indexed: 1.5 TST\n"), "{stdout}");
    assert!(contains_price_value(&stdout).is_none(), "{stdout}");
    assert!(!stdout.contains("status:200"), "{stdout}");
}

#[test]
fn invalid_price_file_exits_with_2() {
    let sample = manifest("tests/fixtures/b1-count.dogstatsd");
    let broken = TempFile::new("not-json");
    let output = binary()
        .args(["count", "--prices", broken.as_str(), &sample])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    assert!(output.stdout.is_empty(), "{}", failure(&output));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert!(stderr.contains("invalid price file"), "{stderr}");
    assert!(stderr.contains("not valid json"), "{stderr}");
    assert!(!stderr.contains("not-json"), "{stderr}");
    assert!(!stderr.contains(ESTIMATE), "{stderr}");

    let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("prices/exists-not.json");
    let output = binary()
        .args([
            "count",
            "--prices",
            missing.to_str().expect("path is utf-8"),
            &sample,
        ])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    assert!(output.stdout.is_empty(), "{}", failure(&output));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert!(stderr.contains("cannot read price file"), "{stderr}");
    assert!(!stderr.contains(ESTIMATE), "{stderr}");
}

#[test]
fn price_file_with_utf8_bom_is_read() {
    let sample = manifest("tests/fixtures/b1-count.dogstatsd");
    let price = TempFile::new(
        "\u{feff}{\"currency\":\"TST\",\"source\":\"https://example.test/source\",\"date\":\"2026-01-02\",\"ingested_per_100\":100}",
    );
    let output = binary()
        .args([
            "count",
            "--plan",
            "pro",
            "--hosts",
            "0",
            "--prices",
            price.as_str(),
            &sample,
        ])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.contains("price: TST per 100\n"), "{stdout}");
}

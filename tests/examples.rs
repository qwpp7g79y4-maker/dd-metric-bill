//! Golden test: the sample files in `examples/` give exactly the output
//! stored next to them.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn golden(args: &[&str], expected: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run the binary");
    assert_eq!(output.status.code(), Some(0), "{args:?}");
    assert!(output.stderr.is_empty(), "{args:?}");
    let want = fs::read_to_string(root().join(expected)).expect("read the expected output");
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8"),
        want,
        "{args:?}"
    );
}

#[test]
fn webshop_dogstatsd_matches_the_golden_output() {
    golden(
        &["count", "examples/webshop.dogstatsd"],
        "examples/webshop.expected.txt",
    );
}

#[test]
fn api_gateway_prometheus_matches_the_golden_output() {
    golden(
        &[
            "count",
            "--format",
            "prometheus",
            "examples/api-gateway.prom",
        ],
        "examples/api-gateway.expected.txt",
    );
}

#[test]
fn page_dogstatsd_example_is_accepted() {
    let output = Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
        .args(["count", "examples/page.dogstatsd"])
        .current_dir(root())
        .output()
        .expect("run the binary");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.is_empty());
}

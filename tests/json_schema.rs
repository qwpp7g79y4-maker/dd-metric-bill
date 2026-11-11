//! Schema test for `--json`: exact key sets and value types, including nested
//! objects. The snapshots pin the values; this pins the shape, so a renamed,
//! dropped or added key fails here with a clear message.

use serde_json::Value;
use std::collections::BTreeSet;
use std::process::Command;

fn run(args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
        .args(args)
        .output()
        .expect("binary start");
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert!(out.stderr.is_empty());
    serde_json::from_slice(&out.stdout).expect("stdout is JSON")
}

fn keys(v: &Value) -> BTreeSet<&str> {
    v.as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect()
}

fn set(list: &[&'static str]) -> BTreeSet<&'static str> {
    list.iter().copied().collect()
}

#[test]
fn count_json_schema() {
    let d = run(&[
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
        "tests/fixtures/b1-count.dogstatsd",
    ]);
    assert_eq!(
        keys(&d),
        set(&[
            "allotment",
            "costs",
            "estimate",
            "indexed",
            "ingested",
            "inputs",
            "metrics",
            "schema_version",
            "source",
            "tags",
            "tool",
            "total",
            "unbounded"
        ])
    );
    assert!(d["schema_version"].is_u64());
    assert!(d["tool"].is_string() && d["estimate"].is_string() && d["source"].is_string());
    assert!(d["inputs"][0].is_string());
    assert!(d["total"].is_u64() && d["ingested"].is_u64() && d["indexed"].is_u64());
    assert!(d["unbounded"].is_array());

    assert_eq!(
        keys(&d["metrics"][0]),
        set(&[
            "combinations",
            "count",
            "indexed",
            "ingested",
            "name",
            "rule",
            "rule_text",
            "type"
        ])
    );
    assert!(d["metrics"][0]["count"].is_u64() && d["metrics"][0]["name"].is_string());

    assert_eq!(keys(&d["tags"][0]), set(&["keys", "name", "type"]));
    assert_eq!(
        keys(&d["tags"][0]["keys"][0]),
        set(&["added", "key", "values"])
    );

    assert_eq!(
        keys(&d["allotment"]),
        set(&[
            "allotment",
            "hosts",
            "indexed",
            "indexed_surplus",
            "ingested",
            "ingested_surplus",
            "plan"
        ])
    );
    assert_eq!(
        keys(&d["costs"]),
        set(&["currency", "date", "lines", "source"])
    );
    assert_eq!(
        keys(&d["costs"]["lines"][0]),
        set(&["above", "amount", "kind"])
    );
    assert!(d["costs"]["lines"][0]["amount"].is_string());
}

#[test]
fn count_json_schema_without_options() {
    let d = run(&["count", "--json", "tests/fixtures/b1-count.dogstatsd"]);
    assert_eq!(
        keys(&d),
        set(&[
            "allotment",
            "estimate",
            "inputs",
            "metrics",
            "schema_version",
            "source",
            "tags",
            "tool",
            "total",
            "unbounded"
        ])
    );
    assert!(d["allotment"].is_null());
}

#[test]
fn diff_json_schema() {
    let d = run(&[
        "diff",
        "--json",
        "tests/fixtures/diff-old.dogstatsd",
        "tests/fixtures/diff-new.dogstatsd",
    ]);
    assert_eq!(
        keys(&d),
        set(&[
            "allotment",
            "estimate",
            "inputs",
            "metrics",
            "schema_version",
            "source",
            "tags",
            "tool",
            "total",
            "unbounded"
        ])
    );
    assert_eq!(keys(&d["inputs"]), set(&["new", "old"]));
    assert_eq!(keys(&d["total"]), set(&["delta", "new", "old"]));
    assert_eq!(
        keys(&d["metrics"][0]),
        set(&["delta", "name", "new", "old", "rule", "type"])
    );
    assert_eq!(keys(&d["tags"][0]), set(&["keys", "name", "type"]));
    assert_eq!(
        keys(&d["tags"][0]["keys"][0]),
        set(&[
            "delta",
            "key",
            "new_added",
            "new_values",
            "old_added",
            "old_values"
        ])
    );
    assert!(d["metrics"][2]["delta"].is_i64());
}

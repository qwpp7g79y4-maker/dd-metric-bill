#![no_main]

use arbitrary::Arbitrary;
use dd_metric_bill::allow::{parse_allow_tags, simulate};
use dd_metric_bill::count::{DEFAULT_HISTOGRAM_AGGREGATES, DEFAULT_HISTOGRAM_PERCENTILES};
use dd_metric_bill_fuzz::{to_samples, FuzzSample};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct AllowCase {
    list_text: String,
    samples: Vec<FuzzSample>,
}

// Allow-list parsing (`--allow-tags`) and the Metrics without Limits
// simulation never panic. Dropping tags can only merge series, so the
// indexed count can never exceed the ingested count, per metric and in
// total.
fuzz_target!(|case: AllowCase| {
    let allow_tags = parse_allow_tags(&case.list_text);
    let samples = to_samples(&case.samples);
    let Ok(report) = simulate(
        &samples,
        Some(&allow_tags),
        DEFAULT_HISTOGRAM_AGGREGATES,
        DEFAULT_HISTOGRAM_PERCENTILES,
        false,
    ) else {
        return;
    };
    let Some(ingested_total) = report.ingested else {
        panic!("simulate with an allow-list must set ingested");
    };
    assert!(
        report.indexed <= ingested_total,
        "indexed {} exceeds ingested {ingested_total}",
        report.indexed
    );
    for metric in &report.metrics {
        let Some(ingested) = metric.ingested else {
            panic!("simulate with an allow-list must set ingested per metric");
        };
        assert!(
            metric.indexed <= ingested,
            "metric {:?} indexed {} exceeds ingested {ingested}",
            metric.name,
            metric.indexed
        );
    }
});

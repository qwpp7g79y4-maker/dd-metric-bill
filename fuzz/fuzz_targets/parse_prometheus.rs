#![no_main]

use dd_metric_bill::parse::parse_prometheus;
use dd_metric_bill_fuzz::run_count_pipeline;
use libfuzzer_sys::fuzz_target;

// Input reading/parsing stage: Prometheus text exposition. Arbitrary
// bytes must never panic, including unterminated label values, unknown
// `# TYPE` families, and metric or label names with multi-byte UTF-8.
fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(samples) = parse_prometheus(text) {
        run_count_pipeline(&samples);
    }
});

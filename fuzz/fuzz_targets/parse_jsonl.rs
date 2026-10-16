#![no_main]

use dd_metric_bill::parse::parse_jsonl;
use dd_metric_bill_fuzz::run_count_pipeline;
use libfuzzer_sys::fuzz_target;

// Input reading/parsing stage: JSON lines. Arbitrary bytes must never
// panic, including a `\uXXXX` escape, a duplicate key, or a value that
// spans lines.
fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(samples) = parse_jsonl(text) {
        run_count_pipeline(&samples);
    }
});

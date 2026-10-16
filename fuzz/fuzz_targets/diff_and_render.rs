#![no_main]

use arbitrary::Arbitrary;
use dd_metric_bill_fuzz::{run_diff_pipeline, to_samples, FuzzSample};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct DiffCase {
    old: Vec<FuzzSample>,
    new: Vec<FuzzSample>,
}

// `diff` and render to human text and JSON, built directly from arbitrary
// old and new samples.
fuzz_target!(|case: DiffCase| {
    run_diff_pipeline(&to_samples(&case.old), &to_samples(&case.new));
});

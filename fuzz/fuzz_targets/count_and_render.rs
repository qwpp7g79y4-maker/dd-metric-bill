#![no_main]

use arbitrary::Arbitrary;
use dd_metric_bill_fuzz::{run_count_pipeline, to_samples, FuzzSample};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct CountCase {
    samples: Vec<FuzzSample>,
}

// `count` and render to human text and JSON, built directly from arbitrary
// samples rather than through a text parser. Catches render bugs that
// need a `Sample` shape no text parser produces.
fuzz_target!(|case: CountCase| {
    run_count_pipeline(&to_samples(&case.samples));
});

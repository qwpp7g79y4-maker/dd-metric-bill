#![no_main]

use dd_metric_bill::parse::parse_dogstatsd;
use dd_metric_bill_fuzz::run_count_pipeline;
use libfuzzer_sys::fuzz_target;

// Input reading/parsing stage: DogStatsD text. Arbitrary bytes, as a file
// read from disk would be, must never panic. A sample that parses is then
// run through `count` and `render` so a parser-produced [`Sample`] is
// checked against the same invariants as a fuzzed one.
fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(samples) = parse_dogstatsd(text) {
        run_count_pipeline(&samples);
    }
});

//! Property tests, run with normal `cargo test`.
//!
//! Modest case counts so this stays fast on every `cargo test` run; the
//! fuzz/ crate (run separately, under `cargo +nightly fuzz run`) covers the
//! same ground with far more cases and a persistent corpus.

use std::path::Path;

use proptest::prelude::*;

use dd_metric_bill::cost::{estimate_cost, parse_prices};
use dd_metric_bill::count::{
    count_by_metric, DEFAULT_HISTOGRAM_AGGREGATES, DEFAULT_HISTOGRAM_PERCENTILES,
};
use dd_metric_bill::diff::diff_samples;
use dd_metric_bill::parse::{parse_dogstatsd, parse_jsonl, parse_prometheus, Kind, Sample, Tag};
use dd_metric_bill::render::{render_count, render_count_json, render_diff, render_diff_json};
use dd_metric_bill::tags::{tag_breakdown, unbounded_tags, DEFAULT_UNBOUNDED_THRESHOLD};

fn small_string() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<char>(), 0..6).prop_map(|chars| chars.into_iter().collect())
}

fn kind_strategy() -> impl Strategy<Value = Kind> {
    prop_oneof![
        Just(Kind::Count),
        Just(Kind::Gauge),
        Just(Kind::Histogram),
        Just(Kind::Distribution),
        Just(Kind::Untyped),
    ]
}

fn tag_strategy() -> impl Strategy<Value = Tag> {
    (small_string(), prop::option::of(small_string())).prop_map(|(key, value)| Tag { key, value })
}

fn sample_strategy() -> impl Strategy<Value = Sample> {
    (
        small_string(),
        kind_strategy(),
        prop::collection::vec(tag_strategy(), 0..5),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(name, kind, tags, d10, counter_total)| Sample {
            name,
            kind,
            tags,
            d10,
            counter_total,
        })
}

fn samples_strategy(max: usize) -> impl Strategy<Value = Vec<Sample>> {
    prop::collection::vec(sample_strategy(), 0..max)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Arbitrary bytes, decoded as UTF-8 text, never panic any of the three
    /// format parsers, however malformed the text is.
    #[test]
    fn parsers_never_panic_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..80)) {
        if let Ok(text) = std::str::from_utf8(&bytes) {
            let _ = parse_dogstatsd(text);
            let _ = parse_jsonl(text);
            let _ = parse_prometheus(text);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// `amount` (`above` metrics at an integer price per 100) matches plain
    /// integer-division arithmetic: `above * price / 100`, written as a
    /// whole number, or with a fraction when the division is not exact.
    #[test]
    fn cost_amount_matches_reference_arithmetic(above in 0u64..=1_000_000, price in 0u64..=1_000_000) {
        let json = format!(
            r#"{{"currency":"TST","source":"s","date":"2026-01-01","ingested_per_100":{price}}}"#
        );
        let prices = parse_prices(&json).expect("an integer price parses");
        let report = estimate_cost(&prices, Some(0), Some(above), 0);
        let amount = &report.lines[0].amount;
        prop_assert_eq!(amount, &reference_amount(above, price));
    }
}

/// `above * price / 100`, as the lowest terms decimal `amount` promises:
/// a whole number, or a whole number and a fraction without a trailing zero.
fn reference_amount(above: u64, price: u64) -> String {
    let numerator = u128::from(above) * u128::from(price);
    let whole = numerator / 100;
    let rest = u64::try_from(numerator % 100).expect("remainder of /100 fits in u64");
    if rest == 0 {
        return whole.to_string();
    }
    let frac = format!("{rest:02}");
    let trimmed = frac.trim_end_matches('0');
    format!("{whole}.{trimmed}")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(150))]

    /// Rendering the same count report twice gives the same text and the
    /// same JSON, and that JSON always parses.
    #[test]
    fn render_count_is_stable_and_json_parses(samples in samples_strategy(8)) {
        let report = count_by_metric(
            &samples,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
        )
        .expect("small samples do not overflow the count");
        let tags = tag_breakdown(&samples, None);
        let unbounded = unbounded_tags(&samples, DEFAULT_UNBOUNDED_THRESHOLD);

        let text_a = render_count(&report, true, None, &tags, &unbounded, None, None);
        let text_b = render_count(&report, true, None, &tags, &unbounded, None, None);
        prop_assert_eq!(&text_a, &text_b);

        let json_a = render_count_json(
            &[Path::new("a.sample")],
            &report,
            None,
            &tags,
            &unbounded,
            None,
            None,
        );
        let json_b = render_count_json(
            &[Path::new("a.sample")],
            &report,
            None,
            &tags,
            &unbounded,
            None,
            None,
        );
        prop_assert_eq!(&json_a, &json_b);
        prop_assert!(serde_json::from_str::<serde_json::Value>(&json_a).is_ok());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// `diff` renders stable text and JSON, the JSON parses, and the total
    /// delta matches the sum of the per-metric deltas.
    #[test]
    fn render_diff_is_stable_and_json_parses(
        old in samples_strategy(5),
        new in samples_strategy(5),
    ) {
        let report = diff_samples(
            &old,
            &new,
            DEFAULT_HISTOGRAM_AGGREGATES,
            DEFAULT_HISTOGRAM_PERCENTILES,
            false,
            None,
        )
        .expect("small samples do not overflow the diff");

        let text_a = render_diff(&report);
        let text_b = render_diff(&report);
        prop_assert_eq!(&text_a, &text_b);

        let json_text = render_diff_json(Path::new("old.sample"), Path::new("new.sample"), &report);
        let doc: serde_json::Value =
            serde_json::from_str(&json_text).expect("rendered diff json must parse");
        let delta = doc["total"]["delta"].as_i64().expect("delta is an i64");
        let sum: i64 = doc["metrics"]
            .as_array()
            .expect("metrics is an array")
            .iter()
            .map(|metric| metric["delta"].as_i64().expect("delta is an i64"))
            .sum();
        prop_assert_eq!(delta, sum);
    }
}

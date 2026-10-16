#![no_main]

use arbitrary::Arbitrary;
use dd_metric_bill::cost::{estimate_cost, parse_prices};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct PriceCase {
    json_text: String,
    allotment: Option<u64>,
    ingested: Option<u64>,
    indexed: u64,
}

// Price file parsing and money/price/cost calculation. A price file never
// panics on arbitrary text, including a duplicate key, a `\uXXXX` escape,
// or a number such as `1e309`. A cost `amount` is always a plain
// non-negative decimal: never negative, `NaN`, or infinite, and 0 when
// usage does not exceed the allotment.
fuzz_target!(|case: PriceCase| {
    let Ok(prices) = parse_prices(&case.json_text) else {
        return;
    };
    let report = estimate_cost(&prices, case.allotment, case.ingested, case.indexed);
    for line in &report.lines {
        assert_valid_amount(&line.amount);
        if let Some(allotment) = case.allotment {
            let usage = match line.kind {
                dd_metric_bill::cost::CostKind::Ingested => case.ingested.unwrap_or(0),
                dd_metric_bill::cost::CostKind::Indexed => case.indexed,
            };
            if usage <= allotment {
                assert_eq!(line.amount, "0", "usage under the allotment must cost 0");
            }
        }
    }
});

// A plain decimal: optional `.` with digits on both sides, never a sign,
// exponent, `NaN`, or `Inf`.
fn assert_valid_amount(amount: &str) {
    assert!(!amount.is_empty(), "amount is empty");
    assert!(!amount.starts_with('-'), "amount is negative: {amount:?}");
    let (whole, frac) = match amount.split_once('.') {
        Some((whole, frac)) => (whole, Some(frac)),
        None => (amount, None),
    };
    assert!(
        !whole.is_empty() && whole.bytes().all(|byte| byte.is_ascii_digit()),
        "amount whole part is not plain digits: {amount:?}"
    );
    if let Some(frac) = frac {
        assert!(
            !frac.is_empty() && frac.bytes().all(|byte| byte.is_ascii_digit()),
            "amount fraction is not plain digits: {amount:?}"
        );
        assert!(
            !frac.ends_with('0'),
            "amount keeps a trailing zero: {amount:?}"
        );
    }
}

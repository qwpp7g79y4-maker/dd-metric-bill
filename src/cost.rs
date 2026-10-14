use std::fmt;

use serde_json::{Map, Value};

/// Twelve digits after the decimal point. More precision does not fit the calculation.
const MAX_SCALE: u32 = 12;

/// Upper bound of the numerator. Together with a usage in `u64`, the product stays within `i128`.
const MAX_NUMERATOR: i128 = 1_000_000_000_000_000_000;

const FIELDS: &[&str] = &[
    "currency",
    "source",
    "date",
    "ingested_per_100",
    "indexed_per_100",
];

/// Price per 100 metrics: `numerator / 10^scale`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PricePer100 {
    numerator: i128,
    scale: u32,
}

/// Price file that was read. The amounts come only from that file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prices {
    pub currency: String,
    pub source: String,
    pub date: String,
    ingested_per_100: PricePer100,
    indexed_per_100: Option<PricePer100>,
}

/// Which volume the line prices (D6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostKind {
    Ingested,
    Indexed,
}

impl CostKind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            CostKind::Ingested => "ingested",
            CostKind::Indexed => "indexed",
        }
    }
}

/// One cost line above the allotment.
///
/// `above` is the usage minus the allotment, or 0 when the usage is lower.
/// `amount` is that part times the price per 100, divided by 100.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostLine {
    pub kind: CostKind,
    pub above: u64,
    pub amount: String,
}

/// Source, date, and the cost lines that belong to a price file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostReport {
    pub currency: String,
    pub source: String,
    pub date: String,
    pub lines: Vec<CostLine>,
}

/// Error in the contents of a price file. The CLI exit code is 2.
#[derive(Debug, PartialEq, Eq)]
pub struct PriceError {
    reason: Reason,
}

#[derive(Debug, PartialEq, Eq)]
enum Reason {
    Json,
    NotObject,
    Missing(&'static str),
    Empty(&'static str),
    NotText(&'static str),
    NotNumber(&'static str),
    Negative(&'static str),
    TooLarge(&'static str),
    Unknown,
    DuplicateKey,
}

impl PriceError {
    fn new(reason: Reason) -> Self {
        Self { reason }
    }

    #[must_use]
    pub fn is_duplicate_key(&self) -> bool {
        matches!(self.reason, Reason::DuplicateKey)
    }
}

impl fmt::Display for PriceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.reason {
            Reason::Json => f.write_str("not valid json"),
            Reason::NotObject => f.write_str("not an object"),
            Reason::Missing(field) => write!(f, "missing field: {field}"),
            Reason::Empty(field) => write!(f, "empty field: {field}"),
            Reason::NotText(field) => write!(f, "field is not text: {field}"),
            Reason::NotNumber(field) => write!(f, "field is not a number: {field}"),
            Reason::Negative(field) => write!(f, "field is negative: {field}"),
            Reason::TooLarge(field) => write!(f, "field is too large: {field}"),
            Reason::Unknown => f.write_str("unknown field"),
            Reason::DuplicateKey => f.write_str("price file has a duplicate key"),
        }
    }
}

impl std::error::Error for PriceError {}

#[derive(Debug, PartialEq, Eq)]
enum PriceNumberError {
    NotNumber,
    Negative,
    TooLarge,
}

/// Reads a price file from JSON.
///
/// Required: `currency`, `source`, `date`, `ingested_per_100`.
/// `indexed_per_100` may be absent. An unknown field, an empty text field,
/// a negative price, or a price that does not fit the calculation is invalid.
/// A UTF-8 BOM at the start is skipped, as in samples.
///
/// A price is the decimal text of the JSON number (not a binary float),
/// stored as an `i128` numerator over `10^scale`. The cost stays in that
/// integer form: usage above the allotment, times the numerator, divided
/// by `10^(scale + 2)`.
///
/// A key repeated at the top level is invalid. The message does not include
/// the file text. A `\uXXXX` escape that decodes to a key already present
/// counts. The same name inside a nested value does not.
pub fn parse_prices(text: &str) -> Result<Prices, PriceError> {
    let text = crate::parse::without_bom(text);
    let value: Value = serde_json::from_str(text).map_err(|_| PriceError::new(Reason::Json))?;
    let Value::Object(obj) = value else {
        return Err(PriceError::new(Reason::NotObject));
    };
    if crate::parse::repeats_top_level_key(text, obj.len()) {
        return Err(PriceError::new(Reason::DuplicateKey));
    }
    let currency = require_text(&obj, "currency")?;
    let source = require_text(&obj, "source")?;
    let date = require_text(&obj, "date")?;
    let ingested_per_100 = require_price(&obj, "ingested_per_100")?;
    let indexed_per_100 = opt_price(&obj, "indexed_per_100")?;
    if obj.keys().any(|key| !FIELDS.contains(&key.as_str())) {
        return Err(PriceError::new(Reason::Unknown));
    }
    Ok(Prices {
        currency,
        source,
        date,
        ingested_per_100,
        indexed_per_100,
    })
}

/// Cost above the allotment (B1).
///
/// `allotment` is `None` without `--plan`; then there are no cost lines.
/// `ingested` is `None` without an allowlist: only configured metrics
/// count toward ingested (D6). Indexed counts only when the file
/// contains `indexed_per_100`. Both volumes get the same allotment, separately (D7).
#[must_use]
pub fn estimate_cost(
    prices: &Prices,
    allotment: Option<u64>,
    ingested: Option<u64>,
    indexed: u64,
) -> CostReport {
    let mut lines = Vec::new();
    if let Some(allotment) = allotment {
        if let Some(usage) = ingested {
            lines.push(cost_line(
                CostKind::Ingested,
                usage,
                allotment,
                prices.ingested_per_100,
            ));
        }
        if let Some(price) = prices.indexed_per_100 {
            lines.push(cost_line(CostKind::Indexed, indexed, allotment, price));
        }
    }
    CostReport {
        currency: prices.currency.clone(),
        source: prices.source.clone(),
        date: prices.date.clone(),
        lines,
    }
}

fn cost_line(kind: CostKind, usage: u64, allotment: u64, price: PricePer100) -> CostLine {
    let above = usage.saturating_sub(allotment);
    CostLine {
        kind,
        above,
        amount: amount(above, price),
    }
}

/// `above` metrics times the price per 100, divided by 100.
fn amount(above: u64, price: PricePer100) -> String {
    let numerator = i128::from(above)
        .checked_mul(price.numerator)
        .expect("price fits after the bound in parse_prices");
    let scale = price.scale + 2;
    let divisor = 10i128
        .checked_pow(scale)
        .expect("scale fits after the bound in parse_prices");
    let whole = numerator / divisor;
    let rest = numerator % divisor;
    if rest == 0 {
        return whole.to_string();
    }
    let width = usize::try_from(scale).expect("scale fits in usize");
    let mut fraction = rest.to_string();
    while fraction.len() < width {
        fraction.insert(0, '0');
    }
    let end = fraction.trim_end_matches('0').len();
    fraction.truncate(end);
    format!("{whole}.{fraction}")
}

fn require_text(obj: &Map<String, Value>, field: &'static str) -> Result<String, PriceError> {
    match obj.get(field) {
        None => Err(PriceError::new(Reason::Missing(field))),
        Some(Value::String(text)) => {
            let text = text.trim();
            if text.is_empty() {
                Err(PriceError::new(Reason::Empty(field)))
            } else {
                Ok(text.to_string())
            }
        }
        Some(_) => Err(PriceError::new(Reason::NotText(field))),
    }
}

fn require_price(obj: &Map<String, Value>, field: &'static str) -> Result<PricePer100, PriceError> {
    match obj.get(field) {
        None => Err(PriceError::new(Reason::Missing(field))),
        Some(Value::Number(number)) => {
            // `arbitrary_precision` makes this the JSON lexical form.
            parse_price(&number.to_string()).map_err(|error| price_error(field, error))
        }
        Some(_) => Err(PriceError::new(Reason::NotNumber(field))),
    }
}

fn opt_price(
    obj: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<PricePer100>, PriceError> {
    match obj.get(field) {
        None => Ok(None),
        Some(Value::Number(number)) => parse_price(&number.to_string())
            .map(Some)
            .map_err(|error| price_error(field, error)),
        Some(_) => Err(PriceError::new(Reason::NotNumber(field))),
    }
}

fn price_error(field: &'static str, error: PriceNumberError) -> PriceError {
    let reason = match error {
        PriceNumberError::NotNumber => Reason::NotNumber(field),
        PriceNumberError::Negative => Reason::Negative(field),
        PriceNumberError::TooLarge => Reason::TooLarge(field),
    };
    PriceError::new(reason)
}

fn parse_price(raw: &str) -> Result<PricePer100, PriceNumberError> {
    if raw.starts_with('-') {
        return Err(PriceNumberError::Negative);
    }
    let (mant, exp) = match raw.find(['e', 'E']) {
        Some(index) => {
            let exp: i32 = raw[index + 1..]
                .parse()
                .map_err(|_| PriceNumberError::NotNumber)?;
            (&raw[..index], exp)
        }
        None => (raw, 0i32),
    };
    if mant.is_empty() {
        return Err(PriceNumberError::NotNumber);
    }
    let (whole, frac) = match mant.split_once('.') {
        Some((whole, frac)) => (whole, frac),
        None => (mant, ""),
    };
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !frac.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(PriceNumberError::NotNumber);
    }
    let mut digits = String::with_capacity(whole.len() + frac.len());
    digits.push_str(whole);
    digits.push_str(frac);
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Ok(PricePer100 {
            numerator: 0,
            scale: 0,
        });
    }
    if digits.len() > 38 {
        return Err(PriceNumberError::TooLarge);
    }
    let mut numerator: i128 = digits.parse().map_err(|_| PriceNumberError::TooLarge)?;
    let frac_len = i32::try_from(frac.len()).map_err(|_| PriceNumberError::TooLarge)?;
    let mut scale = frac_len
        .checked_sub(exp)
        .ok_or(PriceNumberError::TooLarge)?;
    if scale < 0 {
        let shift = u32::try_from(-scale).map_err(|_| PriceNumberError::TooLarge)?;
        numerator = times_ten(numerator, shift)?;
        scale = 0;
    }
    let mut scale = u32::try_from(scale).map_err(|_| PriceNumberError::TooLarge)?;
    while scale > 0 && numerator % 10 == 0 {
        numerator /= 10;
        scale -= 1;
    }
    if scale > MAX_SCALE || numerator > MAX_NUMERATOR {
        return Err(PriceNumberError::TooLarge);
    }
    Ok(PricePer100 { numerator, scale })
}

fn times_ten(mut numerator: i128, mut times: u32) -> Result<i128, PriceNumberError> {
    while times > 0 {
        numerator = numerator
            .checked_mul(10)
            .ok_or(PriceNumberError::TooLarge)?;
        times -= 1;
    }
    Ok(numerator)
}

#[cfg(test)]
mod tests {
    use super::{estimate_cost, parse_prices, CostKind};

    fn must_parse(json: &str) -> super::Prices {
        parse_prices(json).unwrap_or_else(|err| panic!("expected a valid price file: {err}"))
    }

    fn price_json(ingested: &str, indexed: &str) -> String {
        let indexed = if indexed.is_empty() {
            String::new()
        } else {
            format!(",\"indexed_per_100\":{indexed}")
        };
        format!(
            r#"{{"currency":"TST","source":"source-test","date":"2026-01-02","ingested_per_100":{ingested}{indexed}}}"#
        )
    }

    #[test]
    fn cost_only_above_the_allotment() {
        let prices = must_parse(&price_json("2", ""));
        let under = estimate_cost(&prices, Some(100), Some(40), 40);
        assert_eq!(under.lines.len(), 1);
        assert_eq!(under.lines[0].kind, CostKind::Ingested);
        assert_eq!(under.lines[0].above, 0);
        assert_eq!(under.lines[0].amount, "0");

        let equal = estimate_cost(&prices, Some(100), Some(100), 100);
        assert_eq!(equal.lines[0].above, 0);
        assert_eq!(equal.lines[0].amount, "0");

        let above = estimate_cost(&prices, Some(100), Some(150), 150);
        assert_eq!(above.lines[0].above, 50);
        assert_eq!(above.lines[0].amount, "1");
    }

    #[test]
    fn ingested_and_indexed_share_one_allotment() {
        let prices = must_parse(&price_json("2", "4"));
        let report = estimate_cost(&prices, Some(100), Some(120), 80);
        assert_eq!(report.lines.len(), 2);
        assert_eq!(report.lines[0].kind, CostKind::Ingested);
        assert_eq!(report.lines[0].above, 20);
        assert_eq!(report.lines[0].amount, "0.4");
        assert_eq!(report.lines[1].kind, CostKind::Indexed);
        assert_eq!(report.lines[1].above, 0);
        assert_eq!(report.lines[1].amount, "0");

        let swapped = estimate_cost(&prices, Some(100), Some(10), 250);
        assert_eq!(swapped.lines[0].above, 0);
        assert_eq!(swapped.lines[0].amount, "0");
        assert_eq!(swapped.lines[1].above, 150);
        assert_eq!(swapped.lines[1].amount, "6");
    }

    #[test]
    fn no_cost_lines_without_an_allotment() {
        let prices = must_parse(&price_json("2", "4"));
        let report = estimate_cost(&prices, None, Some(500), 500);
        assert!(report.lines.is_empty());
        assert_eq!(report.currency, "TST");
        assert_eq!(report.source, "source-test");
        assert_eq!(report.date, "2026-01-02");
    }

    #[test]
    fn no_ingested_line_without_an_allowlist() {
        let prices = must_parse(&price_json("2", "4"));
        let report = estimate_cost(&prices, Some(10), None, 40);
        assert_eq!(report.lines.len(), 1);
        assert_eq!(report.lines[0].kind, CostKind::Indexed);
        assert_eq!(report.lines[0].above, 30);
        assert_eq!(report.lines[0].amount, "1.2");
    }

    #[test]
    fn no_indexed_line_without_an_indexed_price() {
        let prices = must_parse(&price_json("2", ""));
        let report = estimate_cost(&prices, Some(0), Some(4), 3);
        assert_eq!(report.lines.len(), 1);
        assert_eq!(report.lines[0].kind, CostKind::Ingested);
        assert_eq!(report.lines[0].above, 4);
        assert_eq!(report.lines[0].amount, "0.08");
    }

    #[test]
    fn price_zero_gives_amount_zero() {
        let prices = must_parse(&price_json("0", "0"));
        let report = estimate_cost(&prices, Some(0), Some(80), 80);
        assert_eq!(report.lines[0].above, 80);
        assert_eq!(report.lines[0].amount, "0");
        assert_eq!(report.lines[1].amount, "0");
    }

    #[test]
    fn fraction_per_hundred_divides_exactly() {
        let prices = must_parse(&price_json("1.25", "0.5"));
        let report = estimate_cost(&prices, Some(0), Some(1), 50);
        assert_eq!(report.lines[0].amount, "0.0125");
        assert_eq!(report.lines[1].above, 50);
        assert_eq!(report.lines[1].amount, "0.25");

        let one = must_parse(&price_json("2", ""));
        let report = estimate_cost(&one, Some(100), Some(101), 0);
        assert_eq!(report.lines[0].above, 1);
        assert_eq!(report.lines[0].amount, "0.02");
    }

    #[test]
    fn text_fields_are_trimmed_and_indexed_may_be_absent() {
        let prices = must_parse(
            r#"{"date":" 2026-01-02 ","source":" source-test ","currency":" TST ","ingested_per_100":2}"#,
        );
        assert!(prices.indexed_per_100.is_none());
        assert_eq!(prices.currency, "TST");
        assert_eq!(prices.source, "source-test");
        assert_eq!(prices.date, "2026-01-02");
    }

    #[test]
    fn every_invalid_price_file() {
        let valid =
            r#""currency":"TST","source":"source-test","date":"2026-01-02","ingested_per_100":2"#;
        let cases = [
            ("empty", String::new(), "not valid json"),
            ("truncated", "{".to_string(), "not valid json"),
            ("text-not-json", "nope".to_string(), "not valid json"),
            ("array", "[]".to_string(), "not an object"),
            ("null", "null".to_string(), "not an object"),
            ("text", "\"x\"".to_string(), "not an object"),
            ("number", "1".to_string(), "not an object"),
            (
                "without-currency",
                r#"{"source":"source-test","date":"2026-01-02","ingested_per_100":2}"#.to_string(),
                "missing field: currency",
            ),
            (
                "without-source",
                r#"{"currency":"TST","date":"2026-01-02","ingested_per_100":2}"#.to_string(),
                "missing field: source",
            ),
            (
                "without-date",
                r#"{"currency":"TST","source":"source-test","ingested_per_100":2}"#.to_string(),
                "missing field: date",
            ),
            (
                "without-ingested",
                r#"{"currency":"TST","source":"source-test","date":"2026-01-02"}"#.to_string(),
                "missing field: ingested_per_100",
            ),
            (
                "currency-empty",
                format!("{{{valid}}}",).replace("\"TST\"", "\"\""),
                "empty field: currency",
            ),
            (
                "currency-wit",
                format!("{{{valid}}}").replace("\"TST\"", "\"   \""),
                "empty field: currency",
            ),
            (
                "source-empty",
                format!("{{{valid}}}").replace("\"source-test\"", "\"\""),
                "empty field: source",
            ),
            (
                "date-empty",
                format!("{{{valid}}}").replace("\"2026-01-02\"", "\" \""),
                "empty field: date",
            ),
            (
                "currency-number",
                format!("{{{valid}}}").replace("\"TST\"", "1"),
                "field is not text: currency",
            ),
            (
                "source-null",
                format!("{{{valid}}}").replace("\"source-test\"", "null"),
                "field is not text: source",
            ),
            (
                "date-bool",
                format!("{{{valid}}}").replace("\"2026-01-02\"", "true"),
                "field is not text: date",
            ),
            (
                "ingested-text",
                format!("{{{valid}}}")
                    .replace("\"ingested_per_100\":2", "\"ingested_per_100\":\"2\""),
                "field is not a number: ingested_per_100",
            ),
            (
                "ingested-null",
                format!("{{{valid}}}")
                    .replace("\"ingested_per_100\":2", "\"ingested_per_100\":null"),
                "field is not a number: ingested_per_100",
            ),
            (
                "ingested-bool",
                format!("{{{valid}}}")
                    .replace("\"ingested_per_100\":2", "\"ingested_per_100\":true"),
                "field is not a number: ingested_per_100",
            ),
            (
                "ingested-object",
                format!("{{{valid}}}").replace("\"ingested_per_100\":2", "\"ingested_per_100\":{}"),
                "field is not a number: ingested_per_100",
            ),
            (
                "ingested-array",
                format!("{{{valid}}}").replace("\"ingested_per_100\":2", "\"ingested_per_100\":[]"),
                "field is not a number: ingested_per_100",
            ),
            (
                "ingested-negative",
                format!("{{{valid}}}").replace("\"ingested_per_100\":2", "\"ingested_per_100\":-1"),
                "field is negative: ingested_per_100",
            ),
            (
                "ingested-too-large",
                format!("{{{valid}}}").replace(
                    "\"ingested_per_100\":2",
                    "\"ingested_per_100\":10000000000000000000",
                ),
                "field is too large: ingested_per_100",
            ),
            (
                "indexed-text",
                format!("{{{valid},\"indexed_per_100\":\"4\"}}"),
                "field is not a number: indexed_per_100",
            ),
            (
                "indexed-null",
                format!("{{{valid},\"indexed_per_100\":null}}"),
                "field is not a number: indexed_per_100",
            ),
            (
                "indexed-negative",
                format!("{{{valid},\"indexed_per_100\":-2}}"),
                "field is negative: indexed_per_100",
            ),
            (
                "indexed-too-large",
                format!("{{{valid},\"indexed_per_100\":10000000000000000000}}"),
                "field is too large: indexed_per_100",
            ),
            (
                "unknown-field",
                format!("{{{valid},\"extra\":1}}"),
                "unknown field",
            ),
            (
                "duplicate-currency",
                "{\"currency\":\"AAA\",\"currency\":\"BBB\",\"source\":\"s\",\"date\":\"2026-10-07\",\"ingested_per_100\":1}".to_string(),
                "price file has a duplicate key",
            ),
            (
                "nested-repeat",
                "{\"currency\":\"AAA\",\"source\":\"s\",\"date\":\"2026-10-07\",\"ingested_per_100\":1,\"extra\":{\"currency\":\"BBB\",\"currency\":\"CCC\"}}".to_string(),
                "unknown field",
            ),
        ];
        for (name, json, expected) in cases {
            let err = match parse_prices(&json) {
                Err(err) => err.to_string(),
                Ok(_) => panic!("{name} should be rejected: {json}"),
            };
            assert_eq!(err, expected, "{name}");
        }
    }
}

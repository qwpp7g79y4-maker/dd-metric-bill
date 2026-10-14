//! Top-level JSON object keys.
//!
//! `serde_json` keeps the last value when a key is repeated. Counting the
//! keys in the text and comparing that with the parsed map catches the
//! repeat, including a `\uXXXX` escape that decodes to a key already present.
//! Nested objects are skipped, so the same name there is not a repeat.

use std::iter::Peekable;
use std::str::Chars;

/// `true` when `text` is a JSON object with a repeated key at the top level.
///
/// `unique_keys` is the number of keys in the object `serde_json` parsed from
/// the same text. A nested repeat does not count. When the text cannot be
/// walked, this returns `true` so a repeat is not kept by accident.
#[must_use]
pub(crate) fn repeats_top_level_key(text: &str, unique_keys: usize) -> bool {
    match count_top_level_keys(text) {
        Some(found) => found > unique_keys,
        None => true,
    }
}

fn count_top_level_keys(text: &str) -> Option<usize> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut chars = text.chars().peekable();
    skip_ws(&mut chars);
    if chars.next() != Some('{') {
        return None;
    }
    skip_ws(&mut chars);
    if chars.peek() == Some(&'}') {
        chars.next();
        skip_ws(&mut chars);
        return if chars.next().is_none() {
            Some(0)
        } else {
            None
        };
    }
    let mut count = 0usize;
    loop {
        skip_ws(&mut chars);
        if !skip_string(&mut chars) {
            return None;
        }
        count += 1;
        skip_ws(&mut chars);
        if chars.next() != Some(':') {
            return None;
        }
        if !skip_value(&mut chars) {
            return None;
        }
        skip_ws(&mut chars);
        match chars.next() {
            Some(',') => {}
            Some('}') => {
                skip_ws(&mut chars);
                return if chars.next().is_none() {
                    Some(count)
                } else {
                    None
                };
            }
            _ => return None,
        }
    }
}

fn skip_ws(chars: &mut Peekable<Chars<'_>>) {
    while matches!(chars.peek(), Some(' ' | '\t' | '\n' | '\r')) {
        chars.next();
    }
}

fn skip_string(chars: &mut Peekable<Chars<'_>>) -> bool {
    if chars.next() != Some('"') {
        return false;
    }
    finish_string(chars)
}

fn finish_string(chars: &mut Peekable<Chars<'_>>) -> bool {
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if escaped {
            if c == 'u' && !skip_hex4(chars) {
                return false;
            }
            escaped = false;
            continue;
        }
        match c {
            '"' => return true,
            '\\' => escaped = true,
            _ => {}
        }
    }
    false
}

fn skip_hex4(chars: &mut Peekable<Chars<'_>>) -> bool {
    for _ in 0..4 {
        match chars.next() {
            Some(c) if c.is_ascii_hexdigit() => {}
            _ => return false,
        }
    }
    true
}

fn skip_value(chars: &mut Peekable<Chars<'_>>) -> bool {
    skip_ws(chars);
    match chars.peek().copied() {
        Some('"') => skip_string(chars),
        Some('{') => skip_container(chars, '{', '}'),
        Some('[') => skip_container(chars, '[', ']'),
        Some('t') => skip_literal(chars, "true"),
        Some('f') => skip_literal(chars, "false"),
        Some('n') => skip_literal(chars, "null"),
        Some('-' | '0'..='9') => skip_number(chars),
        _ => false,
    }
}

fn skip_container(chars: &mut Peekable<Chars<'_>>, open: char, close: char) -> bool {
    if chars.next() != Some(open) {
        return false;
    }
    let mut depth = 1usize;
    while let Some(c) = chars.next() {
        if c == '"' {
            if !finish_string(chars) {
                return false;
            }
        } else if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return true;
            }
        }
    }
    false
}

fn skip_literal(chars: &mut Peekable<Chars<'_>>, literal: &str) -> bool {
    for expected in literal.chars() {
        if chars.next() != Some(expected) {
            return false;
        }
    }
    true
}

fn skip_number(chars: &mut Peekable<Chars<'_>>) -> bool {
    if chars.peek() == Some(&'-') {
        chars.next();
    }
    if !skip_digits(chars) {
        return false;
    }
    if chars.peek() == Some(&'.') {
        chars.next();
        if !skip_digits(chars) {
            return false;
        }
    }
    if matches!(chars.peek(), Some('e' | 'E')) {
        chars.next();
        if matches!(chars.peek(), Some('+' | '-')) {
            chars.next();
        }
        if !skip_digits(chars) {
            return false;
        }
    }
    true
}

fn skip_digits(chars: &mut Peekable<Chars<'_>>) -> bool {
    let mut any = false;
    while matches!(chars.peek(), Some(c) if c.is_ascii_digit()) {
        any = true;
        chars.next();
    }
    any
}

#[cfg(test)]
mod tests {
    use super::repeats_top_level_key;

    fn repeats(text: &str) -> bool {
        let value: serde_json::Value = serde_json::from_str(text).expect("json");
        let n = value.as_object().expect("object").len();
        repeats_top_level_key(text, n)
    }

    #[test]
    fn literal_inputs_and_nested_values() {
        assert!(repeats(
            r#"{"metric":"m","type":"count","type":"histogram"}"#
        ));
        assert!(repeats(
            r#"{"currency":"AAA","currency":"BBB","source":"s","date":"2026-10-07","ingested_per_100":1}"#
        ));
        assert!(repeats(
            "{\"metric\":\"m\",\"type\":\"count\",\"typ\\u0065\":\"histogram\"}"
        ));
        assert!(!repeats(
            r#"{"metric":"m","type":"count","value":{"type":"histogram","type":"count"}}"#
        ));
        assert!(!repeats(
            r#"{"metric":"m","type":"count","value":{"metric":"n"}}"#
        ));
        assert!(!repeats(r#"{"a":"\"}{","b":1}"#));
        assert!(!repeats(r#"{"a":[1,{"a":1,"a":2}],"b":3}"#));
        let pretty = "{\n\t\"currency\"\t:\t\"AAA\",\n\t\"source\"\t:\t\"s\",\n\t\"date\"\t:\t\"2026-10-07\",\n\t\"ingested_per_100\"\t:\t1\n}\n";
        assert!(!repeats(pretty));
        let example = include_str!("../../prices/datadog-example.json");
        assert!(!repeats(example));
    }
}

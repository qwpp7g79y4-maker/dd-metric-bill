use super::{Kind, ParseError, Sample, Tag};

#[derive(Clone, Copy)]
enum Fail {
    Json,
    Shape,
    Name,
    Type,
    Tag,
}

impl Fail {
    fn text(self) -> &'static str {
        match self {
            Fail::Json => "not valid json",
            Fail::Shape => "shape is wrong",
            Fail::Name => "name is missing",
            Fail::Type => "type is missing or unknown",
            Fail::Tag => "tag is missing or invalid",
        }
    }
}

fn fail(line: usize, reason: Fail) -> ParseError {
    ParseError::new("jsonl", line, reason.text())
}

/// Parses JSONL into samples.
///
/// One object per line: `{"metric","type","tags"}`.
/// `type` is `count`, `gauge`, `histogram`, or `distribution`.
/// `tags` is a list of `key:value` or `key` without a value.
/// Other fields, such as a value, are ignored. Empty lines are ignored too.
/// A UTF-8 BOM at the start is skipped.
pub fn parse_jsonl(input: &str) -> Result<Vec<Sample>, ParseError> {
    let input = super::without_bom(input);
    let mut samples = Vec::new();
    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        samples.push(parse_line(line, line_no)?);
    }
    Ok(samples)
}

fn parse_line(line: &str, line_no: usize) -> Result<Sample, ParseError> {
    let value: serde_json::Value =
        serde_json::from_str(line).map_err(|_| fail(line_no, Fail::Json))?;
    let Some(obj) = value.as_object() else {
        return Err(fail(line_no, Fail::Shape));
    };
    let name = match obj.get("metric") {
        Some(serde_json::Value::String(name)) => {
            let name = name.trim();
            if name.is_empty() {
                return Err(fail(line_no, Fail::Name));
            }
            name.to_string()
        }
        _ => return Err(fail(line_no, Fail::Name)),
    };
    let kind = match obj.get("type").and_then(serde_json::Value::as_str) {
        Some(token) => kind_of(token.trim()).ok_or_else(|| fail(line_no, Fail::Type))?,
        None => return Err(fail(line_no, Fail::Type)),
    };
    let tags = tags_of(obj.get("tags"), line_no)?;
    Ok(Sample {
        name,
        kind,
        tags,
        d10: false,
        counter_total: false,
    })
}

fn kind_of(token: &str) -> Option<Kind> {
    match token {
        "count" => Some(Kind::Count),
        "gauge" => Some(Kind::Gauge),
        "histogram" => Some(Kind::Histogram),
        "distribution" => Some(Kind::Distribution),
        _ => None,
    }
}

fn tags_of(value: Option<&serde_json::Value>, line_no: usize) -> Result<Vec<Tag>, ParseError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(fail(line_no, Fail::Tag));
    };
    let mut tags = Vec::with_capacity(items.len());
    for item in items {
        let Some(piece) = item.as_str() else {
            return Err(fail(line_no, Fail::Tag));
        };
        tags.push(tag_of(piece).map_err(|_| fail(line_no, Fail::Tag))?);
    }
    Ok(tags)
}

fn tag_of(piece: &str) -> Result<Tag, Fail> {
    let piece = piece.trim();
    if piece.is_empty() {
        return Err(Fail::Tag);
    }
    let (key, value) = match piece.split_once(':') {
        Some((key, value)) => {
            let key = key.trim();
            if key.is_empty() {
                return Err(Fail::Tag);
            }
            let value = value.trim();
            let value = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            };
            (key.to_string(), value)
        }
        None => (piece.to_string(), None),
    };
    Ok(Tag { key, value })
}

#[cfg(test)]
mod tests {
    use super::{parse_jsonl, Kind, Tag};

    #[test]
    fn every_type_and_tags() {
        let input = r#"
{"metric":"page.views","type":"count","tags":["host:a","env"],"value":1}
{"metric":"queue.depth","type":"gauge","tags":["debug:"]}
{"metric":"rpc.latency","type":"histogram"}
{"metric":"payload.bytes","type":"distribution","tags":["status:200"]}
"#;
        let samples = parse_jsonl(input).expect("valid sample");
        let got: Vec<_> = samples
            .iter()
            .map(|sample| (sample.name.as_str(), sample.kind))
            .collect();
        assert_eq!(
            got,
            vec![
                ("page.views", Kind::Count),
                ("queue.depth", Kind::Gauge),
                ("rpc.latency", Kind::Histogram),
                ("payload.bytes", Kind::Distribution),
            ]
        );
        assert_eq!(
            samples[0].tags,
            vec![
                Tag {
                    key: "host".to_string(),
                    value: Some("a".to_string()),
                },
                Tag {
                    key: "env".to_string(),
                    value: None,
                },
            ]
        );
        assert_eq!(
            samples[1].tags,
            vec![Tag {
                key: "debug".to_string(),
                value: None,
            }]
        );
        assert!(samples[2].tags.is_empty());
    }

    #[test]
    fn three_invalid_lines() {
        assert_invalid("{\"metric\":\"canary-42\"", 1, &["canary-42"]);
        assert_invalid(
            "{\"metric\":\"ok\",\"type\":\"count\"}\n\n{\"metric\":\"name-canary\",\"type\":\"nope\"}\n",
            3,
            &["name-canary", "nope"],
        );
        assert_invalid(
            "{\"metric\":\"ok\",\"type\":\"count\"}\n{\"metric\":\"m\",\"type\":\"gauge\",\"tags\":[\"token:canary\"]}\n{\"metric\":\"m\",\"type\":\"count\",\"tags\":\"token:canary\"}\n",
            3,
            &["token:canary", "canary"],
        );
    }

    fn assert_invalid(input: &str, line_no: usize, forbidden: &[&str]) {
        let err = parse_jsonl(input).expect_err("expected input_error");
        assert_eq!(err.exit_code(), 2);
        assert_eq!(err.line(), line_no);
        let msg = err.to_string();
        assert!(msg.contains(&format!("line {line_no}")), "{msg}");
        for piece in forbidden {
            assert!(!msg.contains(piece), "message leaks {piece:?}: {msg}");
        }
        for line in input.lines().map(str::trim).filter(|line| !line.is_empty()) {
            assert!(!msg.contains(line), "message contains the rule: {msg}");
        }
    }
}

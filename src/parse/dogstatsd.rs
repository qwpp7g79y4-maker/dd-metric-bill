use super::{Kind, ParseError, Sample, Tag};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reason {
    Shape,
    Name,
    Value,
    Type,
    SampleRate,
    Tag,
    Extra,
}

impl Reason {
    fn text(self) -> &'static str {
        match self {
            Reason::Shape => "shape is wrong",
            Reason::Name => "name is missing",
            Reason::Value => "value is missing or invalid",
            Reason::Type => "type is missing or unknown",
            Reason::SampleRate => "sample rate is missing or invalid",
            Reason::Tag => "tag is missing or invalid",
            Reason::Extra => "unknown field",
        }
    }
}

fn fail(line: usize, reason: Reason) -> ParseError {
    ParseError::new("dogstatsd", line, reason.text())
}

#[derive(Clone, Copy)]
enum Wire {
    Count,
    Gauge,
    Histogram,
    Distribution,
    Timer,
    Set,
}

impl Wire {
    fn kind(self) -> Kind {
        match self {
            Wire::Count => Kind::Count,
            Wire::Gauge | Wire::Set => Kind::Gauge,
            Wire::Histogram | Wire::Timer => Kind::Histogram,
            Wire::Distribution => Kind::Distribution,
        }
    }
}

/// Parses DogStatsD lines into samples.
///
/// Shape: `name:value|type` with type `c`, `g`, `h`, `d`, `ms`, or `s`.
/// After that, `|@rate` and `|#tag:v,tag` may appear, in any order.
/// A tag may have no value (`|#env`). `ms` counts as histogram, `s` as gauge.
/// Empty lines are skipped. A UTF-8 BOM at the start is skipped too.
pub fn parse_dogstatsd(input: &str) -> Result<Vec<Sample>, ParseError> {
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
    let mut parts = line.split('|').map(str::trim);
    let Some(head) = parts.next() else {
        return Err(fail(line_no, Reason::Shape));
    };
    let Some(type_token) = parts.next() else {
        return Err(fail(line_no, Reason::Shape));
    };
    let (name, value) = split_name_value(head).map_err(|reason| fail(line_no, reason))?;
    let wire = parse_wire(type_token).map_err(|reason| fail(line_no, reason))?;
    check_value(wire, value).map_err(|reason| fail(line_no, reason))?;
    let tags = parse_tail(parts, line_no)?;
    Ok(Sample {
        name: name.to_string(),
        kind: wire.kind(),
        tags,
        d10: false,
        counter_total: false,
    })
}

fn split_name_value(head: &str) -> Result<(&str, &str), Reason> {
    let Some((name, value)) = head.split_once(':') else {
        return Err(Reason::Shape);
    };
    let name = name.trim();
    let value = value.trim();
    if name.is_empty() {
        return Err(Reason::Name);
    }
    if value.is_empty() {
        return Err(Reason::Value);
    }
    Ok((name, value))
}

fn parse_wire(token: &str) -> Result<Wire, Reason> {
    match token {
        "c" => Ok(Wire::Count),
        "g" => Ok(Wire::Gauge),
        "h" => Ok(Wire::Histogram),
        "d" => Ok(Wire::Distribution),
        "ms" => Ok(Wire::Timer),
        "s" => Ok(Wire::Set),
        _ => Err(Reason::Type),
    }
}

fn check_value(wire: Wire, value: &str) -> Result<(), Reason> {
    if matches!(wire, Wire::Set) {
        return Ok(());
    }
    match value.parse::<f64>() {
        Ok(number) if number.is_finite() => Ok(()),
        _ => Err(Reason::Value),
    }
}

fn parse_tail<'a>(
    parts: impl Iterator<Item = &'a str>,
    line_no: usize,
) -> Result<Vec<Tag>, ParseError> {
    let mut seen_rate = false;
    let mut seen_tags = false;
    let mut tags = Vec::new();
    for part in parts {
        if let Some(rate) = part.strip_prefix('@') {
            if seen_rate {
                return Err(fail(line_no, Reason::SampleRate));
            }
            check_rate(rate).map_err(|reason| fail(line_no, reason))?;
            seen_rate = true;
        } else if let Some(raw_tags) = part.strip_prefix('#') {
            if seen_tags {
                return Err(fail(line_no, Reason::Tag));
            }
            tags = parse_tags(raw_tags).map_err(|reason| fail(line_no, reason))?;
            seen_tags = true;
        } else {
            return Err(fail(line_no, Reason::Extra));
        }
    }
    Ok(tags)
}

fn check_rate(raw: &str) -> Result<(), Reason> {
    let Ok(rate) = raw.parse::<f64>() else {
        return Err(Reason::SampleRate);
    };
    if rate.is_finite() && rate > 0.0 && rate <= 1.0 {
        Ok(())
    } else {
        Err(Reason::SampleRate)
    }
}

fn parse_tags(raw: &str) -> Result<Vec<Tag>, Reason> {
    if raw.is_empty() {
        return Err(Reason::Tag);
    }
    let mut tags = Vec::new();
    for piece in raw.split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            return Err(Reason::Tag);
        }
        let (key, value) = match piece.split_once(':') {
            Some((key, value)) => {
                let key = key.trim();
                let value = value.trim();
                if key.is_empty() {
                    return Err(Reason::Tag);
                }
                let value = if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                };
                (key.to_string(), value)
            }
            None => (piece.to_string(), None),
        };
        tags.push(Tag { key, value });
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::{parse_dogstatsd, Kind, Tag};

    #[test]
    fn every_type() {
        let input = "\
page.views:1|c
queue.depth:3|g
rpc.latency:10|h
payload.bytes:512|d
rpc.time:10|ms
users.uniques:alice|s
";
        let samples = parse_dogstatsd(input).expect("valid sample");
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
                ("rpc.time", Kind::Histogram),
                ("users.uniques", Kind::Gauge),
            ]
        );
        assert!(samples.iter().all(|sample| sample.tags.is_empty()));
    }

    #[test]
    fn with_sample_rate() {
        let input = "\
checkout.amount:9.5|g|@0.25|#host:a
checkout.amount:1|c|#host:a|@1
";
        let samples = parse_dogstatsd(input).expect("valid sample");
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].kind, Kind::Gauge);
        assert_eq!(samples[1].kind, Kind::Count);
        for sample in &samples {
            assert_eq!(sample.name, "checkout.amount");
            assert_eq!(
                sample.tags,
                vec![Tag {
                    key: "host".to_string(),
                    value: Some("a".to_string()),
                }]
            );
        }
    }

    #[test]
    fn tag_without_value() {
        let samples = parse_dogstatsd("page.hits:1|c|#env,status:200,debug:\n").expect("valid");
        assert_eq!(
            samples[0].tags,
            vec![
                Tag {
                    key: "env".to_string(),
                    value: None,
                },
                Tag {
                    key: "status".to_string(),
                    value: Some("200".to_string()),
                },
                Tag {
                    key: "debug".to_string(),
                    value: None,
                },
            ]
        );
    }

    #[test]
    fn three_invalid_lines() {
        assert_invalid("beta.requests:1", 1, &["beta.requests"]);
        assert_invalid(
            "ok.metric:1|c\nbeta.queue:5|c|@two",
            2,
            &["beta.queue", "two"],
        );
        assert_invalid(
            "page.views:1|c\n\nnope.metric:1|zz|#token:canary",
            3,
            &["nope.metric", "zz", "canary"],
        );
    }

    fn assert_invalid(input: &str, line_no: usize, forbidden: &[&str]) {
        let err = parse_dogstatsd(input).expect_err("expected input_error");
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

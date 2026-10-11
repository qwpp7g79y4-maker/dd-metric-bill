use std::collections::{BTreeMap, BTreeSet};

use crate::parse::{Kind, Sample};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TagPair {
    key: String,
    value: Option<String>,
}

type Combination = BTreeSet<TagPair>;

struct Group {
    kind: Kind,
    combinations: BTreeSet<Combination>,
}

/// One tag key within one metric.
///
/// `unique_values` counts the distinct values. `added` is the number of series
/// with the key minus the number without that key: unique tag combinations
/// (D1, D2) minus what remains when the key is dropped everywhere. The
/// histogram and distribution factor is not included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagKey {
    pub key: String,
    pub unique_values: u64,
    pub added: u64,
}

/// Breakdown of one metric name and kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricTags {
    pub name: String,
    pub kind: Kind,
    pub keys: Vec<TagKey>,
}

/// Per metric, the tag keys, descending by added series.
///
/// On an equal contribution, the number of unique values comes next, then
/// the key name. `top` keeps the first N keys per metric; `None` keeps
/// all of them. A metric without tags is omitted.
#[must_use]
pub fn tag_breakdown(samples: &[Sample], top: Option<u64>) -> Vec<MetricTags> {
    let mut groups: BTreeMap<(String, u8), Group> = BTreeMap::new();
    for sample in samples {
        let key = (sample.name.clone(), kind_ord(sample.kind));
        let group = groups.entry(key).or_insert_with(|| Group {
            kind: sample.kind,
            combinations: BTreeSet::new(),
        });
        group.combinations.insert(tags_of(sample));
    }
    let limit = top.map(|count| usize::try_from(count).expect("top fits in usize"));
    groups
        .into_iter()
        .filter_map(|((name, _), group)| {
            let keys = keys_of(&group.combinations, limit);
            if keys.is_empty() {
                None
            } else {
                Some(MetricTags {
                    name,
                    kind: group.kind,
                    keys,
                })
            }
        })
        .collect()
}

fn keys_of(combinations: &BTreeSet<Combination>, limit: Option<usize>) -> Vec<TagKey> {
    let total = u64::try_from(combinations.len()).expect("number of series fits in u64");
    let mut keys: Vec<TagKey> = key_names(combinations)
        .into_iter()
        .map(|name| contribution(combinations, name, total))
        .collect();
    keys.sort_by(|a, b| {
        b.added
            .cmp(&a.added)
            .then(b.unique_values.cmp(&a.unique_values))
            .then(a.key.cmp(&b.key))
    });
    if let Some(max) = limit {
        keys.truncate(max);
    }
    keys
}

fn key_names(combinations: &BTreeSet<Combination>) -> BTreeSet<&str> {
    combinations
        .iter()
        .flat_map(|combination| combination.iter().map(|tag| tag.key.as_str()))
        .collect()
}

fn contribution(combinations: &BTreeSet<Combination>, name: &str, total: u64) -> TagKey {
    let mut values = BTreeSet::new();
    let mut without = BTreeSet::new();
    for combination in combinations {
        let mut rest = Combination::new();
        for tag in combination {
            if tag.key == name {
                values.insert(tag.value.clone());
            } else {
                rest.insert(tag.clone());
            }
        }
        without.insert(rest);
    }
    let without_n = u64::try_from(without.len()).expect("number of series fits in u64");
    TagKey {
        key: name.to_string(),
        unique_values: u64::try_from(values.len()).expect("number of values fits in u64"),
        added: total
            .checked_sub(without_n)
            .expect("removing a key does not create more series"),
    }
}

fn kind_ord(kind: Kind) -> u8 {
    match kind {
        Kind::Count => 0,
        Kind::Gauge => 1,
        Kind::Histogram => 2,
        Kind::Distribution => 3,
        Kind::Untyped => 4,
    }
}

fn tags_of(sample: &Sample) -> Combination {
    sample
        .tags
        .iter()
        .map(|tag| TagPair {
            key: tag.key.clone(),
            value: tag.value.clone(),
        })
        .collect()
}

/// Default `--unbounded-threshold`. More unique values than this
/// makes a key unbounded.
pub const DEFAULT_UNBOUNDED_THRESHOLD: u64 = 50;

/// Hex id: a contiguous hex string of at least this length.
const HEX_ID_MIN_LEN: usize = 16;

/// Numeric id: digits only, at least this length.
///
/// Shorter is not an id pattern. That leaves the status codes from the
/// B1 example (`200`, `400`) and port numbers (at most 5 digits) alone.
const NUMERIC_ID_MIN_LEN: usize = 6;

/// Each hash part of a pod name (`name-<hash>-<hash>`) is at least this long.
const POD_HASH_MIN_LEN: usize = 5;

/// Why a tag key is unbounded.
///
/// `Threshold` means more unique values than the threshold. The other
/// variants are a value that looks like an id pattern. The order is the
/// order in the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnboundedReason {
    Threshold,
    Uuid,
    HexId,
    NumericId,
    PodName,
}

/// One unbounded tag key, with the reasons that apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnboundedTag {
    pub key: String,
    pub reasons: Vec<UnboundedReason>,
}

/// Unbounded keys of one metric name and kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricUnbounded {
    pub name: String,
    pub kind: Kind,
    pub keys: Vec<UnboundedTag>,
}

struct UnboundedGroup {
    kind: Kind,
    keys: BTreeMap<String, BTreeSet<Option<String>>>,
}

/// Tag keys that are unbounded.
///
/// A key is included when it has more unique values than `threshold`,
/// or when a value looks like a uuid, hex id, numeric id, or pod name.
/// The reasons are returned; the values themselves are not.
#[must_use]
pub fn unbounded_tags(samples: &[Sample], threshold: u64) -> Vec<MetricUnbounded> {
    let mut groups: BTreeMap<(String, u8), UnboundedGroup> = BTreeMap::new();
    for sample in samples {
        let key = (sample.name.clone(), kind_ord(sample.kind));
        let group = groups.entry(key).or_insert_with(|| UnboundedGroup {
            kind: sample.kind,
            keys: BTreeMap::new(),
        });
        for tag in &sample.tags {
            group
                .keys
                .entry(tag.key.clone())
                .or_default()
                .insert(tag.value.clone());
        }
    }
    groups
        .into_iter()
        .filter_map(|((name, _), group)| {
            let keys = unbounded_keys(&group.keys, threshold);
            if keys.is_empty() {
                None
            } else {
                Some(MetricUnbounded {
                    name,
                    kind: group.kind,
                    keys,
                })
            }
        })
        .collect()
}

fn unbounded_keys(
    keys: &BTreeMap<String, BTreeSet<Option<String>>>,
    threshold: u64,
) -> Vec<UnboundedTag> {
    keys.iter()
        .filter_map(|(key, values)| {
            let reasons = reasons_of(values, threshold);
            if reasons.is_empty() {
                None
            } else {
                Some(UnboundedTag {
                    key: key.clone(),
                    reasons,
                })
            }
        })
        .collect()
}

fn reasons_of(values: &BTreeSet<Option<String>>, threshold: u64) -> Vec<UnboundedReason> {
    let mut reasons = BTreeSet::new();
    let unique = u64::try_from(values.len()).expect("number of values fits in u64");
    if unique > threshold {
        reasons.insert(UnboundedReason::Threshold);
    }
    for value in values.iter().flatten() {
        if let Some(pattern) = pattern_of(value) {
            reasons.insert(pattern);
        }
    }
    reasons.into_iter().collect()
}

fn pattern_of(value: &str) -> Option<UnboundedReason> {
    if is_uuid(value) {
        return Some(UnboundedReason::Uuid);
    }
    if is_pod_name(value) {
        return Some(UnboundedReason::PodName);
    }
    if is_number_id(value) {
        return Some(UnboundedReason::NumericId);
    }
    if is_hex_id(value) {
        return Some(UnboundedReason::HexId);
    }
    None
}

fn is_uuid(value: &str) -> bool {
    let mut parts = value.split('-');
    for length in [8, 4, 4, 4, 12] {
        let Some(part) = parts.next() else {
            return false;
        };
        if part.len() != length || !part.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return false;
        }
    }
    parts.next().is_none()
}

fn is_hex_id(value: &str) -> bool {
    value.len() >= HEX_ID_MIN_LEN
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value
            .bytes()
            .any(|byte| matches!(byte, b'a'..=b'f' | b'A'..=b'F'))
}

fn is_number_id(value: &str) -> bool {
    value.len() >= NUMERIC_ID_MIN_LEN && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_pod_name(value: &str) -> bool {
    let Some((heading, hash2)) = value.rsplit_once('-') else {
        return false;
    };
    let Some((name, hash1)) = heading.rsplit_once('-') else {
        return false;
    };
    is_pod_name_part(name) && is_pod_hash(hash1) && is_pod_hash(hash2)
}

fn is_pod_hash(hash: &str) -> bool {
    hash.len() >= POD_HASH_MIN_LEN && hash.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn is_pod_name_part(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::{
        tag_breakdown, unbounded_tags, TagKey, UnboundedReason, DEFAULT_UNBOUNDED_THRESHOLD,
    };
    use crate::parse::{parse_dogstatsd, Kind, Sample};

    /// Same sample as `tests/fixtures/user-id.dogstatsd`.
    const USER_ID: &str = "\
checkout:1|c|#env:prod,endpoint:pay,user_id:u1
checkout:1|c|#env:prod,endpoint:pay,user_id:u2
checkout:1|c|#env:prod,endpoint:pay,user_id:u3
checkout:1|c|#env:prod,endpoint:cart,user_id:u3
";

    fn tuples(keys: &[TagKey]) -> Vec<(&str, u64, u64)> {
        keys.iter()
            .map(|tag| (tag.key.as_str(), tag.unique_values, tag.added))
            .collect()
    }

    #[test]
    fn user_id_adds_the_most_and_comes_first() {
        let samples = parse_dogstatsd(USER_ID).expect("synthetic sample");
        let out = tag_breakdown(&samples, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "checkout");
        assert_eq!(out[0].kind, Kind::Count);
        assert_eq!(
            tuples(&out[0].keys),
            vec![("user_id", 3, 2), ("endpoint", 2, 1), ("env", 1, 0),]
        );
    }

    #[test]
    fn top_keeps_the_first_n_per_metric() {
        let samples = parse_dogstatsd(USER_ID).expect("synthetic sample");
        let out = tag_breakdown(&samples, Some(1));
        assert_eq!(tuples(&out[0].keys), vec![("user_id", 3, 2)]);
    }

    #[test]
    fn temperature_state_adds_0() {
        let input = "\
temperature:60|g|#country:USA,region:Northeast,city:NYC,state:NY
temperature:82|g|#country:USA,region:Southeast,city:Orlando,state:Florida
temperature:84|g|#country:USA,region:Southeast,city:Miami,state:Florida
";
        let samples = parse_dogstatsd(input).expect("synthetic sample");
        let out = tag_breakdown(&samples, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, Kind::Gauge);
        assert_eq!(
            tuples(&out[0].keys),
            vec![
                ("city", 3, 1),
                ("region", 2, 0),
                ("state", 2, 0),
                ("country", 1, 0),
            ]
        );
    }

    #[test]
    fn added_counts_series_not_the_histogram_factor() {
        let input = "\
rpc.latency:1|h|#host:a
rpc.latency:1|h|#host:b
";
        let samples = parse_dogstatsd(input).expect("synthetic sample");
        let out = tag_breakdown(&samples, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, Kind::Histogram);
        assert_eq!(tuples(&out[0].keys), vec![("host", 2, 1)]);
    }

    #[test]
    fn reversed_tag_order_is_the_same_series() {
        let input = "\
m:1|c|#host:a,env:prod
m:1|c|#env:prod,host:a
m:1|c|#host:b,env:prod
";
        let samples = parse_dogstatsd(input).expect("synthetic sample");
        let out = tag_breakdown(&samples, None);
        assert_eq!(tuples(&out[0].keys), vec![("host", 2, 1), ("env", 1, 0)]);
    }

    #[test]
    fn uuid_is_unbounded() {
        let input = "\
m:1|c|#request_id:550e8400-e29b-41d4-a716-446655440000
m:1|c|#request_id:550E8400-E29B-41D4-A716-446655440000
";
        assert_eq!(
            marker(input, DEFAULT_UNBOUNDED_THRESHOLD),
            vec![("request_id".to_string(), vec![UnboundedReason::Uuid])]
        );
    }

    #[test]
    fn hex_id_from_16_characters() {
        let input = "\
m:1|c|#trace:abcdef0123456789
m:1|c|#span:ABCDEF0123456789
m:1|c|#kort:abcdef012345678
";
        assert_eq!(
            marker(input, DEFAULT_UNBOUNDED_THRESHOLD),
            vec![
                ("span".to_string(), vec![UnboundedReason::HexId]),
                ("trace".to_string(), vec![UnboundedReason::HexId]),
            ]
        );
    }

    #[test]
    fn numeric_id_from_six_digits() {
        let input = "m:1|c|#user_id:123456,kort:12345,status:200\n";
        assert_eq!(
            marker(input, DEFAULT_UNBOUNDED_THRESHOLD),
            vec![("user_id".to_string(), vec![UnboundedReason::NumericId])]
        );
    }

    #[test]
    fn pod_name_is_unbounded() {
        let input = "\
m:1|c|#pod:my-api-a1b2c3d4e5-f6g7h
m:1|c|#host:web-1
";
        assert_eq!(
            marker(input, DEFAULT_UNBOUNDED_THRESHOLD),
            vec![("pod".to_string(), vec![UnboundedReason::PodName])]
        );
    }

    #[test]
    fn threshold_50_no_and_51_yes() {
        assert_eq!(DEFAULT_UNBOUNDED_THRESHOLD, 50);
        let fifty = parse_dogstatsd(&buckets(50)).expect("synthetic sample");
        assert!(unbounded_tags(&fifty, 50).is_empty());
        let fifty_one = parse_dogstatsd(&buckets(51)).expect("synthetic sample");
        assert_eq!(
            marker_of(&fifty_one, 50),
            vec![("bucket".to_string(), vec![UnboundedReason::Threshold])]
        );
        assert!(unbounded_tags(&fifty_one, 51).is_empty());
    }

    #[test]
    fn env_prod_is_not_marked() {
        let input = "\
m:1|c|#env:prod,request_id:550e8400-e29b-41d4-a716-446655440000
";
        assert_eq!(
            marker(input, DEFAULT_UNBOUNDED_THRESHOLD),
            vec![("request_id".to_string(), vec![UnboundedReason::Uuid])]
        );
    }

    #[test]
    fn threshold_and_pattern_are_both_named() {
        let input = "m:1|c|#user_id:123456\n";
        assert_eq!(
            marker(input, 0),
            vec![(
                "user_id".to_string(),
                vec![UnboundedReason::Threshold, UnboundedReason::NumericId]
            )]
        );
    }

    fn marker(input: &str, threshold: u64) -> Vec<(String, Vec<UnboundedReason>)> {
        let samples = parse_dogstatsd(input).expect("synthetic sample");
        marker_of(&samples, threshold)
    }

    fn marker_of(samples: &[Sample], threshold: u64) -> Vec<(String, Vec<UnboundedReason>)> {
        unbounded_tags(samples, threshold)
            .into_iter()
            .flat_map(|metric| metric.keys.into_iter().map(|tag| (tag.key, tag.reasons)))
            .collect()
    }

    fn buckets(n: u64) -> String {
        use std::fmt::Write;
        (1..=n).fold(String::new(), |mut out, i| {
            let _ = writeln!(out, "m:1|c|#bucket:v{i}");
            out
        })
    }

    #[test]
    fn name_and_kind_stay_separate() {
        let input = "\
m:1|c|#host:a
m:1|h|#host:a
m:1|h|#host:b
";
        let samples = parse_dogstatsd(input).expect("synthetic sample");
        let out = tag_breakdown(&samples, None);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].kind, Kind::Count);
        assert_eq!(tuples(&out[0].keys), vec![("host", 1, 0)]);
        assert_eq!(out[1].kind, Kind::Histogram);
        assert_eq!(tuples(&out[1].keys), vec![("host", 2, 1)]);
    }
}

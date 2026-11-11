use super::{conflicting_host, Tag};

fn tag(key: &str, value: &str) -> Tag {
    Tag {
        key: key.into(),
        value: Some(value.into()),
    }
}

/// A hand-built `HOST:a` and `host:b` are two values of one key, like parsed tags.
#[test]
fn hand_built_host_keys_differing_in_case_conflict() {
    assert!(conflicting_host(&[tag("HOST", "a"), tag("host", "b")]));
}

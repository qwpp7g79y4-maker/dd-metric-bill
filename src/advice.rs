//! Advice on which unbounded tag keys to drop.
//!
//! For each metric with unbounded keys, the billable count is measured twice:
//! with every tag, and without the unbounded keys. The difference is what
//! dropping those keys saves. The count uses the same rules and factors as
//! `count`, so a histogram saves its factor per series removed.

use std::collections::HashSet;

use crate::count::{count_tags_where, CountError};
use crate::parse::Sample;
use crate::series::lowered;
use crate::tags::{DropAdvice, MetricUnbounded};

/// Fill in [`MetricUnbounded::advice`] for every metric.
///
/// Advice stays `None` when dropping the keys saves nothing.
///
/// # Errors
///
/// The same overflow errors as [`crate::count::count_by_metric`].
pub fn add_advice(
    unbounded: &mut [MetricUnbounded],
    samples: &[Sample],
    histogram_aggregates: u64,
    histogram_percentiles: u64,
    percentiles: bool,
) -> Result<(), CountError> {
    for metric in unbounded {
        let own: Vec<Sample> = samples
            .iter()
            .filter(|sample| sample.name.as_ref() == metric.name && sample.kind == metric.kind)
            .cloned()
            .collect();
        let dropped: HashSet<&str> = metric.keys.iter().map(|tag| tag.key.as_str()).collect();
        let before = count_tags_where(
            &own,
            |_| true,
            histogram_aggregates,
            histogram_percentiles,
            percentiles,
        )?
        .total;
        let after = count_tags_where(
            &own,
            |tag| !dropped.contains(lowered(&tag.key).as_ref()),
            histogram_aggregates,
            histogram_percentiles,
            percentiles,
        )?
        .total;
        let advice = DropAdvice { before, after };
        metric.advice = (advice.saved() > 0).then_some(advice);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::add_advice;
    use crate::parse::{parse_dogstatsd, Kind};
    use crate::tags::unbounded_tags;

    fn advice_for(input: &str, threshold: u64) -> Vec<crate::tags::MetricUnbounded> {
        let samples = parse_dogstatsd(input).expect("valid sample");
        let mut unbounded = unbounded_tags(&samples, threshold);
        add_advice(&mut unbounded, &samples, 4, 1, false).expect("no overflow");
        unbounded
    }

    #[test]
    fn dropping_the_high_cardinality_key_collapses_the_series() {
        let got = advice_for(
            "m:1|c|#env:prod,user_id:a\nm:1|c|#env:prod,user_id:b\nm:1|c|#env:prod,user_id:c\n",
            2,
        );
        assert_eq!(got.len(), 1);
        let advice = got[0].advice.expect("advice");
        assert_eq!((advice.before, advice.after, advice.saved()), (3, 1, 2));
    }

    #[test]
    fn only_unbounded_keys_are_dropped() {
        let got = advice_for(
            "m:1|c|#env:prod,user_id:a\nm:1|c|#env:prod,user_id:b\nm:1|c|#env:dev,user_id:b\nm:1|c|#env:dev,user_id:c\n",
            2,
        );
        // user_id has 3 values; env has 2. Dropping user_id leaves prod and dev.
        let advice = got[0].advice.expect("advice");
        assert_eq!((advice.before, advice.after), (4, 2));
    }

    #[test]
    fn histogram_saves_the_factor_per_series() {
        let got = advice_for("m:1|h|#user_id:a\nm:1|h|#user_id:b\nm:1|h|#user_id:c\n", 2);
        assert_eq!(got[0].kind, Kind::Histogram);
        let advice = got[0].advice.expect("advice");
        assert_eq!((advice.before, advice.after), (15, 5));
    }

    #[test]
    fn keys_in_other_case_are_dropped_too() {
        let got = advice_for("m:1|c|#User_Id:a\nm:1|c|#USER_ID:b\nm:1|c|#user_id:c\n", 2);
        let advice = got[0].advice.expect("advice");
        assert_eq!((advice.before, advice.after), (3, 1));
    }

    #[test]
    fn no_advice_when_nothing_is_unbounded() {
        assert!(advice_for("m:1|c|#env:prod\nm:1|c|#env:dev\n", 50).is_empty());
    }

    #[test]
    fn a_metric_is_advised_on_its_own_samples_only() {
        let got = advice_for(
            "a:1|c|#user_id:1\na:1|c|#user_id:2\na:1|c|#user_id:3\nb:1|c|#user_id:1\nb:1|c|#user_id:2\n",
            2,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "a");
        let advice = got[0].advice.expect("advice");
        assert_eq!((advice.before, advice.after), (3, 1));
    }

    #[test]
    fn single_series_pattern_key_saves_nothing() {
        let got = advice_for(
            "m:1|c|#request_id:550e8400-e29b-41d4-a716-446655440000\n",
            50,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].advice, None);
    }
}

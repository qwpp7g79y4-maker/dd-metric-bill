//! Compact series identity shared by the counting passes.
//!
//! A series is a set of tags. Holding each one as owned strings in a tree of
//! trees costs several hundred bytes per series; interning every distinct
//! string once and keeping a series as a sorted list of id pairs costs a few
//! dozen.

use std::collections::HashMap;
use std::rc::Rc;

use crate::parse::{Sample, Tag};

/// Every distinct string once, addressed by a `u32` id.
#[derive(Default)]
pub(crate) struct Interner {
    ids: HashMap<Rc<str>, u32>,
    texts: Vec<Rc<str>>,
}

impl Interner {
    pub(crate) fn id(&mut self, text: &str) -> u32 {
        if let Some(&id) = self.ids.get(text) {
            return id;
        }
        let id = u32::try_from(self.texts.len()).expect("fewer than 2^32 distinct strings");
        let shared: Rc<str> = Rc::from(text);
        self.texts.push(Rc::clone(&shared));
        self.ids.insert(shared, id);
        id
    }

    pub(crate) fn text(&self, id: u32) -> &str {
        &self.texts[id as usize]
    }
}

/// One tag: the key id and the value id plus one. 0 is a tag without a value.
pub(crate) type Pair = (u32, u32);

/// A series: its tags as a sorted list of unique pairs.
pub(crate) type Combo = Box<[Pair]>;

pub(crate) fn pair(interner: &mut Interner, tag: &Tag) -> Pair {
    let key = interner.id(&tag.key);
    let value = tag
        .value
        .as_deref()
        .map_or(0, |value| interner.id(value) + 1);
    (key, value)
}

/// The tags of `sample` that pass `keep`, as a series.
pub(crate) fn combo_of(
    interner: &mut Interner,
    sample: &Sample,
    keep: impl Fn(&Tag) -> bool,
) -> Combo {
    let mut pairs: Vec<Pair> = sample
        .tags
        .iter()
        .filter(|tag| keep(tag))
        .map(|tag| pair(interner, tag))
        .collect();
    pairs.sort_unstable();
    pairs.dedup();
    pairs.into_boxed_slice()
}

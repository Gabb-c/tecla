//! Embedded word list and random word generation.

use std::sync::LazyLock;

use rand::Rng;
use rand::seq::IndexedRandom;

static ENGLISH: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    include_str!("../assets/words/english.txt")
        .lines()
        .collect()
});

pub fn english() -> &'static [&'static str] {
    &ENGLISH
}

/// Picks `count` words from the English list using `rng`.
pub fn random_words<R: Rng + ?Sized>(rng: &mut R, count: usize) -> Vec<&'static str> {
    let list = english();
    (0..count)
        .filter_map(|_| list.choose(rng).copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;

    #[test]
    fn same_seed_gives_same_sequence() {
        let a = random_words(&mut StdRng::seed_from_u64(42), 50);
        let b = random_words(&mut StdRng::seed_from_u64(42), 50);
        assert_eq!(a, b);
        assert_eq!(a.len(), 50);
    }

    #[test]
    fn list_has_no_blank_or_padded_entries() {
        let list = english();
        assert!(list.len() >= 150);
        for word in list {
            assert!(!word.trim().is_empty(), "blank entry");
            assert!(
                !word.contains(char::is_whitespace),
                "whitespace in {word:?}"
            );
        }
    }
}

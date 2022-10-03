use crate::bip39::WORDS as BIP39_WORDS;
use rand::Rng;
use std::{fmt::Display, str::FromStr};

#[derive(Debug)]
pub struct TarId {
    prefix: u16,
    words: [u16; 4],
}

impl Display for TarId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:04}-{}-{}-{}-{}",
            self.prefix,
            BIP39_WORDS[self.words[0] as usize],
            BIP39_WORDS[self.words[1] as usize],
            BIP39_WORDS[self.words[2] as usize],
            BIP39_WORDS[self.words[3] as usize]
        )
    }
}

impl TarId {
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        let prefix = rng.gen_range(0..10000);
        let words = [
            rng.gen_range(0..2048),
            rng.gen_range(0..2048),
            rng.gen_range(0..2048),
            rng.gen_range(0..2048),
        ];
        Self { prefix, words }
    }

    pub fn to_string(&self) -> String {
        format!("{}", self)
    }

    pub fn parse(input: &str) -> Option<Self> {
        let mut input = input.split('-');
        let num = input.next()?.parse().ok()?;

        let mut words = [0; 4];
        for i in 0..4 {
            let word = input.next()?;
            match BIP39_WORDS.binary_search(&word) {
                Ok(idx) => words[i] = idx as u16,
                Err(_) if word.len() <= 10 && word.len() >= 2 => {
                    let lower = word.to_lowercase();
                    let candidates: Vec<_> = BIP39_WORDS
                        .iter()
                        .enumerate()
                        .filter(|(_, w)| levenshtein::levenshtein(&lower, w) <= 1)
                        .map(|(id, _)| id)
                        .collect();

                    if candidates.len() == 1 {
                        words[i] = candidates[0] as u16;
                    } else {
                        return None;
                    }
                }
                Err(_) => {
                    return None;
                }
            }
        }

        // Trailing Words
        if input.next().is_some() {
            return None;
        }

        Some(TarId { prefix: num, words })
    }
}

impl FromStr for TarId {
    type Err = ();
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        TarId::parse(input).ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bip39::WORDS as BIP39_WORDS;

    #[test]
    fn bip39_are_sorted() {
        let mut sorted = BIP39_WORDS.to_vec();
        sorted.sort();
        assert_eq!(BIP39_WORDS.to_vec(), sorted);
    }

    #[test]
    fn test_parse() {
        let id = TarId::parse("0005-abandon-ability-able-about").unwrap();
        assert_eq!(id.prefix, 5);
        assert_eq!(id.words, [0, 1, 2, 3]);

        assert_eq!(id.to_string(), "0005-abandon-ability-able-about")
    }

    #[test]
    fn test_parse_err() {
        let id = TarId::parse("0005-abondon-abilty-able-abou").unwrap();
        assert_eq!(id.prefix, 5);
        assert_eq!(id.words, [0, 1, 2, 3]);
    }
}

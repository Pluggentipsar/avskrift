//! Transcript words → CTC label ids. Numbers are spelled out as spoken Swedish, foreign
//! letters folded to the model's alphabet, punctuation dropped.

use std::collections::HashMap;

use anyhow::{anyhow, Result};

pub struct Vocab {
    ids: HashMap<char, u32>,
    chars: HashMap<u32, char>,
    pub blank: u32,
    pub size: usize,
}

impl Vocab {
    /// Parse a wav2vec2 `vocab.json` ({"<pad>": 0, "A": 7, ...}); `<pad>` is the CTC blank.
    pub fn from_json(json: &str) -> Result<Self> {
        let map: HashMap<String, u32> = serde_json::from_str(json)?;
        let blank = *map.get("<pad>").ok_or_else(|| anyhow!("vocab saknar <pad>"))?;
        let size = map.values().max().map_or(0, |m| *m as usize + 1);
        let ids: HashMap<char, u32> = map
            .iter()
            .filter_map(|(k, v)| {
                let mut chars = k.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) if c != '|' => Some((c, *v)),
                    _ => None,
                }
            })
            .collect();
        let chars = ids.iter().map(|(c, id)| (*id, *c)).collect();
        Ok(Self { ids, chars, blank, size })
    }

    /// The letter a label stands for (None for blank and special tokens).
    pub fn char_of(&self, id: u32) -> Option<char> {
        self.chars.get(&id).copied()
    }

    /// Label ids for one transcript word. Empty when nothing is pronounceable (e.g. "–").
    pub fn encode(&self, word: &str) -> Vec<u32> {
        spoken(word).chars().flat_map(char::to_uppercase).filter_map(|c| self.ids.get(&fold(c)).copied()).collect()
    }
}

/// Map letters outside the model alphabet to the closest Swedish one.
fn fold(c: char) -> char {
    match c {
        'À' | 'Á' | 'Â' | 'Ã' => 'A',
        'Æ' => 'Ä',
        'Ø' | 'Œ' => 'Ö',
        'Ü' => 'Y',
        'Ç' => 'C',
        'È' | 'Ê' | 'Ë' => 'E',
        'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
        'Ñ' => 'N',
        'Ò' | 'Ó' | 'Ô' | 'Õ' => 'O',
        'Ù' | 'Ú' | 'Û' => 'U',
        'Ý' => 'Y',
        other => other,
    }
}

/// Replace digit runs and a few symbols with their spoken Swedish form.
pub fn spoken(word: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = word.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            let digits: String = chars[i..j].iter().collect();
            match digits.parse::<u64>() {
                // Leading zeros ("007") and very long runs are read digit by digit.
                Ok(n) if !(digits.len() > 1 && digits.starts_with('0')) && n < 1_000_000_000 => {
                    out.push_str(&number(n))
                }
                _ => digits.chars().for_each(|d| out.push_str(&number(d.to_digit(10).unwrap_or(0) as u64))),
            }
            // Decimal comma between digits: "3,5" → "tre komma fem".
            if j + 1 < chars.len() && (chars[j] == ',' || chars[j] == '.') && chars[j + 1].is_ascii_digit() {
                out.push_str("komma");
                j += 1;
            }
            i = j;
            continue;
        }
        match c {
            '%' => out.push_str("procent"),
            '&' => out.push_str("och"),
            '+' => out.push_str("plus"),
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// Swedish cardinal number as one word, as it is usually read aloud ("tjugoett", "hundrafem").
pub fn number(n: u64) -> String {
    const ONES: [&str; 20] = [
        "noll", "ett", "två", "tre", "fyra", "fem", "sex", "sju", "åtta", "nio", "tio", "elva", "tolv", "tretton",
        "fjorton", "femton", "sexton", "sjutton", "arton", "nitton",
    ];
    const TENS: [&str; 10] = ["", "", "tjugo", "trettio", "fyrtio", "femtio", "sextio", "sjuttio", "åttio", "nittio"];
    fn below_thousand(n: u64, out: &mut String) {
        let (h, rest) = (n / 100, n % 100);
        if h > 1 {
            out.push_str(ONES[h as usize]);
        }
        if h > 0 {
            out.push_str("hundra");
        }
        if rest >= 20 {
            out.push_str(TENS[(rest / 10) as usize]);
            if rest % 10 > 0 {
                out.push_str(ONES[(rest % 10) as usize]);
            }
        } else if rest > 0 || h == 0 {
            out.push_str(ONES[rest as usize]);
        }
    }
    let mut out = String::new();
    let (millions, thousands, rest) = (n / 1_000_000, (n / 1000) % 1000, n % 1000);
    if millions > 0 {
        if millions == 1 {
            out.push_str("enmiljon");
        } else {
            below_thousand(millions, &mut out);
            out.push_str("miljoner");
        }
    }
    if thousands > 0 {
        if thousands > 1 {
            below_thousand(thousands, &mut out);
        }
        out.push_str("tusen");
    }
    if rest > 0 || out.is_empty() {
        below_thousand(rest, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_spelled_as_spoken() {
        assert_eq!(number(0), "noll");
        assert_eq!(number(12), "tolv");
        assert_eq!(number(21), "tjugoett");
        assert_eq!(number(105), "hundrafem");
        assert_eq!(number(250), "tvåhundrafemtio");
        assert_eq!(number(1000), "tusen");
        assert_eq!(number(2026), "tvåtusentjugosex");
        assert_eq!(number(3_000_000), "tremiljoner");
    }

    #[test]
    fn spoken_handles_symbols_decimals_and_leading_zeros() {
        assert_eq!(spoken("12"), "tolv");
        assert_eq!(spoken("3,5"), "trekommafem");
        assert_eq!(spoken("50%"), "femtioprocent");
        assert_eq!(spoken("007"), "nollnollsju");
        assert_eq!(spoken("AI-projekt."), "AI-projekt.");
    }

    #[test]
    fn encode_uses_model_alphabet() {
        let v = Vocab::from_json(r#"{"<pad>":0,"|":4,"A":7,"I":11,"Y":29,"Ä":22,"T":5,"O":14,"L":12,"V":19}"#).unwrap();
        assert_eq!(v.blank, 0);
        assert_eq!(v.encode("AI!"), [7, 11]);
        assert_eq!(v.encode("tolv"), v.encode("12"));
        assert_eq!(v.encode("ü"), [29]);
        assert!(v.encode("–").is_empty());
    }
}

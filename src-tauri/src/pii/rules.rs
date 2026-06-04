//! Rule-based detectors for structured PII that the NER model does not reliably catch.

use once_cell::sync::Lazy;
use regex::Regex;

use super::{char_after_is_alnum, char_before_is_alnum, Category, Source, Span};

static PNR: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d{6,8}[-+]?\d{4}").unwrap());
static EMAIL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").unwrap());
static PHONE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?:\+46|0046|0)[\d \-]{6,12}\d").unwrap());
static IPV4: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}").unwrap());
static URL: Lazy<Regex> = Lazy::new(|| Regex::new(r#"(?:https?://|www\.)[^\s<>"'()]+"#).unwrap());
static ICD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[A-Z][0-9]{2}(?:\.[0-9]{1,2}[A-Z]?)?").unwrap());
static LGH: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\blgh\.?\s*\d{1,4}\b").unwrap());
// Street address: an optional directional prefix, a capitalised stem ending in a Swedish
// street suffix, then a house number (the precision anchor). E.g. "Storgatan 12B",
// "Norra Kungsvägen 3".
static GATA: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"(?:(?:Norra|Södra|Östra|Västra|Gamla|Nya|Lilla|Stora|Övre|Nedre) )?",
        r"\p{Lu}\p{Ll}*",
        r"(?:gatan|gata|vägen|väg|gränden|gränd|stigen|stig|torget|torg|allén|allé|backen|plan)",
        // House number; an optional single letter ("12B"/"12 B"). The \b stops it from
        // swallowing the first letter of the next word ("Storvägen 7 och" -> "…7").
        r"\s+\d{1,4}(?:\s?[A-Za-z]\b)?",
    ))
    .unwrap()
});
// School name: a capitalised stem ending in a Swedish school suffix, optional genitive -s.
// E.g. "Björkskolan", "Vasagymnasiet", "Montessoriförskolans".
static SKOLA: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"\p{Lu}\p{Ll}+",
        r"(?:gymnasieskolan|grundskolan|gymnasiet|förskolan|särskolan|friskolan|skolan)",
        r"s?",
    ))
    .unwrap()
});

/// Swedish personnummer (10/12 digits, optional `-`/`+`), validated by date + Luhn checksum.
/// Also accepts samordningsnummer (day +60).
pub fn personnummer(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    for m in PNR.find_iter(text) {
        let (s, e) = (m.start(), m.end());
        if char_before_is_alnum(text, s) || char_after_is_alnum(text, e) {
            continue;
        }
        let digits: Vec<u8> = m.as_str().bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
        let ten: [u8; 10] = match digits.len() {
            10 => digits[..].try_into().unwrap(),
            12 => digits[2..].try_into().unwrap(),
            _ => continue,
        };
        // A de-identification tool must mask anything that *looks* like a personnummer — even if the
        // Luhn checksum fails (made-up/test numbers, OCR or transcription slips). A plausible date
        // keeps us from masking arbitrary digit runs; Luhn only adjusts the confidence score.
        if valid_pnr_date(&ten) {
            let score = if luhn_valid(&ten) { 1.0 } else { 0.6 };
            out.push(Span::new(s, e, m.as_str(), Category::Personnummer, Source::Rule, score));
        }
    }
    out
}

fn valid_pnr_date(t: &[u8; 10]) -> bool {
    let month = t[2] * 10 + t[3];
    let day = t[4] * 10 + t[5];
    (1..=12).contains(&month) && ((1..=31).contains(&day) || (61..=91).contains(&day))
}

/// Luhn (mod-10) checksum over the first 9 of the 10 digits; last digit is the check digit.
fn luhn_valid(t: &[u8; 10]) -> bool {
    let mut sum = 0u32;
    for (i, &d) in t[..9].iter().enumerate() {
        let mut v = d as u32 * if i % 2 == 0 { 2 } else { 1 };
        if v > 9 {
            v -= 9;
        }
        sum += v;
    }
    (10 - (sum % 10)) % 10 == t[9] as u32
}

pub fn epost(text: &str) -> Vec<Span> {
    EMAIL
        .find_iter(text)
        .map(|m| Span::new(m.start(), m.end(), m.as_str(), Category::Epost, Source::Rule, 1.0))
        .collect()
}

/// Swedish phone numbers starting with `0`, `+46` or `0046`. Heuristic; reviewed by the user.
pub fn telefon(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    for m in PHONE.find_iter(text) {
        let (s, e) = (m.start(), m.end());
        if char_before_is_alnum(text, s) || char_after_is_alnum(text, e) {
            continue;
        }
        let n = m.as_str().bytes().filter(u8::is_ascii_digit).count();
        if (8..=13).contains(&n) {
            out.push(Span::new(s, e, m.as_str(), Category::Telefon, Source::Rule, 0.9));
        }
    }
    out
}

pub fn ip_adress(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    for m in IPV4.find_iter(text) {
        let (s, e) = (m.start(), m.end());
        let before = text[..s].chars().next_back();
        let after = text[e..].chars().next();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '.')
            || after.is_some_and(|c| c.is_alphanumeric() || c == '.')
        {
            continue;
        }
        if m.as_str().split('.').all(|o| o.parse::<u16>().map(|v| v <= 255).unwrap_or(false)) {
            out.push(Span::new(s, e, m.as_str(), Category::IpAdress, Source::Rule, 0.9));
        }
    }
    out
}

/// Web addresses (`https://…`, `http://…`, `www.…`). Trailing sentence punctuation that the
/// greedy match swallowed is trimmed back off the span.
pub fn url(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    for m in URL.find_iter(text) {
        let s = m.start();
        let mut e = m.end();
        while text[s..e]
            .ends_with(|c: char| matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '\''))
        {
            e -= 1;
        }
        if e > s {
            out.push(Span::new(s, e, &text[s..e], Category::Url, Source::Rule, 0.95));
        }
    }
    out
}

/// ICD-10 diagnosis codes, e.g. `F90.0`, `F84`, `J45.9`.
pub fn icd10(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    for m in ICD.find_iter(text) {
        let (s, e) = (m.start(), m.end());
        if char_before_is_alnum(text, s) || char_after_is_alnum(text, e) {
            continue;
        }
        out.push(Span::new(s, e, m.as_str(), Category::Diagnos, Source::Rule, 0.85));
    }
    out
}

/// Apartment number, e.g. "lgh 1203" — part of an address.
pub fn lagenhet(text: &str) -> Vec<Span> {
    LGH.find_iter(text)
        .map(|m| Span::new(m.start(), m.end(), m.as_str(), Category::Plats, Source::Rule, 0.9))
        .collect()
}

/// Swedish street address ("Storgatan 12B"). The trailing house number keeps precision high.
pub fn gatuadress(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    for m in GATA.find_iter(text) {
        let s = m.start();
        if char_before_is_alnum(text, s) {
            continue;
        }
        out.push(Span::new(s, m.end(), m.as_str(), Category::Plats, Source::Rule, 0.85));
    }
    out
}

/// Swedish school name by suffix ("Björkskolan", "Vasagymnasiet").
pub fn skolnamn(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    for m in SKOLA.find_iter(text) {
        let (s, e) = (m.start(), m.end());
        if char_before_is_alnum(text, s) || char_after_is_alnum(text, e) {
            continue;
        }
        out.push(Span::new(s, e, m.as_str(), Category::Plats, Source::Rule, 0.8));
    }
    out
}

/// Run every rule detector over the text.
pub fn all(text: &str) -> Vec<Span> {
    let mut v = personnummer(text);
    v.extend(epost(text));
    v.extend(telefon(text));
    v.extend(ip_adress(text));
    v.extend(url(text));
    v.extend(icd10(text));
    v.extend(lagenhet(text));
    v.extend(gatuadress(text));
    v.extend(skolnamn(text));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_personnummer_with_dash() {
        let s = personnummer("Patienten 811228-9874 skrevs in.");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].category, Category::Personnummer);
    }

    #[test]
    fn valid_personnummer_12_digits() {
        let s = personnummer("19811228-9874");
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn masks_pnr_shape_even_if_checksum_fails() {
        // Format + a plausible date is enough to mask (don't leak made-up/OCR-broken numbers),
        // but at lower confidence than a Luhn-valid number.
        let s = personnummer("811228-9870");
        assert_eq!(s.len(), 1);
        assert!(s[0].score < 1.0);
        // The example that slipped through before this fix:
        assert_eq!(personnummer("(personnummer 120504-1234,").len(), 1);
    }

    #[test]
    fn ignores_digit_runs_without_a_valid_date() {
        // 10–12 digits but month "99" → not personnummer-shaped, leave it alone.
        assert!(personnummer("ordernr 999999-0000 bokfört").is_empty());
    }

    #[test]
    fn rejects_glued_to_word() {
        assert!(personnummer("ref8112289874x").is_empty());
    }

    #[test]
    fn finds_email() {
        let s = epost("kontakt anna.svensson@example.se nu");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].category, Category::Epost);
    }

    #[test]
    fn finds_phone() {
        let s = telefon("ring 070-123 45 67 imorgon");
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn finds_ipv4_but_not_version() {
        assert_eq!(ip_adress("server 192.168.0.1 svarar").len(), 1);
        assert!(ip_adress("version 1.2.3.4.5 finns").is_empty());
    }

    #[test]
    fn finds_icd_codes() {
        let s = icd10("Diagnos F90.0 samt J45 noterades");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].category, Category::Diagnos);
    }

    #[test]
    fn icd_not_glued_to_word() {
        assert!(icd10("kod ABC12 finns").is_empty());
    }

    #[test]
    fn finds_url_and_trims_trailing_punctuation() {
        let s = url("Besök https://exempel.se/sida. Eller www.test.se, sade hen.");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].text, "https://exempel.se/sida"); // trailing "." trimmed
        assert_eq!(s[0].category, Category::Url);
        assert_eq!(s[1].text, "www.test.se"); // trailing "," trimmed
    }

    #[test]
    fn finds_street_address() {
        let s = gatuadress("Hen bor på Storgatan 12B numera.");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].text, "Storgatan 12B");
        assert_eq!(s[0].category, Category::Plats);
    }

    #[test]
    fn finds_street_address_with_directional_prefix() {
        let s = gatuadress("Adressen är Norra Kungsvägen 3.");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].text, "Norra Kungsvägen 3");
    }

    #[test]
    fn street_needs_a_house_number() {
        // A street suffix alone is not an address (the model handles bare place names).
        assert!(gatuadress("Vi gick längs gatan.").is_empty());
    }

    #[test]
    fn finds_school_name_incl_genitive() {
        assert_eq!(skolnamn("Hen går på Björkskolan i höst.").len(), 1);
        let gen = skolnamn("Vasagymnasiets rektor ringde.");
        assert_eq!(gen.len(), 1);
        assert_eq!(gen[0].text, "Vasagymnasiets");
        assert_eq!(gen[0].category, Category::Plats);
    }

    #[test]
    fn bare_school_word_is_not_matched() {
        assert!(skolnamn("Eleven trivs i skolan.").is_empty());
    }
}

pub mod dictionary;
pub mod gazetteer;
pub mod merge;
pub mod model;
pub mod pseudonym;
pub mod rules;

use serde::{Deserialize, Serialize};

/// A category of detectable personal data. Serialized as snake_case for the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Person,
    Plats,
    Organisation,
    Tid,
    Handelse,
    Personnummer,
    Telefon,
    Epost,
    IpAdress,
    Url,
    Diagnos,
    Medicin,
    Egen,
    Ovrigt,
}

impl Category {
    /// Human-readable Swedish label, also used as the pseudonym stem ("Person 1").
    pub fn label(self) -> &'static str {
        match self {
            Category::Person => "Person",
            Category::Plats => "Plats",
            Category::Organisation => "Organisation",
            Category::Tid => "Tid",
            Category::Handelse => "Händelse",
            Category::Personnummer => "Personnummer",
            Category::Telefon => "Telefon",
            Category::Epost => "E-post",
            Category::IpAdress => "IP-adress",
            Category::Url => "Webbadress",
            Category::Diagnos => "Diagnos",
            Category::Medicin => "Medicin",
            Category::Egen => "Egen",
            Category::Ovrigt => "Övrigt",
        }
    }

    pub const ALL: [Category; 14] = [
        Category::Person,
        Category::Plats,
        Category::Organisation,
        Category::Tid,
        Category::Handelse,
        Category::Personnummer,
        Category::Telefon,
        Category::Epost,
        Category::IpAdress,
        Category::Url,
        Category::Diagnos,
        Category::Medicin,
        Category::Egen,
        Category::Ovrigt,
    ];
}

/// Which detector produced a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Model,
    Rule,
    Dictionary,
    Ai,
    Manual,
}

/// A detected span, with byte offsets into the original UTF-8 text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub category: Category,
    pub source: Source,
    pub score: f32,
    /// User-supplied replacement that overrides the automatic pseudonym (manual masks only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<String>,
}

impl Span {
    pub fn new(start: usize, end: usize, text: &str, category: Category, source: Source, score: f32) -> Self {
        Span { start, end, text: text.to_string(), category, source, score, custom: None }
    }

    /// A span the user created by clicking a word, with an optional free-text replacement.
    pub fn manual(start: usize, end: usize, text: &str, category: Category, custom: Option<String>) -> Self {
        Span { start, end, text: text.to_string(), category, source: Source::Manual, score: 1.0, custom }
    }
}

/// True if the position `i` in `text` is at a word boundary on the given side.
/// Used by rule and dictionary detectors to avoid matching inside a larger token.
pub(crate) fn char_before_is_alnum(text: &str, i: usize) -> bool {
    text[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric())
}

pub(crate) fn char_after_is_alnum(text: &str, i: usize) -> bool {
    text[i..].chars().next().is_some_and(|c| c.is_alphanumeric())
}

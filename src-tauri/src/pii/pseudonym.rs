//! Consistent pseudonymisation: the same surface form (per category) always maps to the
//! same label, e.g. every "Anna Svensson" becomes "Person 1" throughout the document.

use std::collections::HashMap;

use super::Category;

#[derive(Default)]
pub struct Pseudonymizer {
    counters: HashMap<Category, usize>,
    mapping: HashMap<(Category, String), String>,
}

impl Pseudonymizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stable replacement label for a category + surface text. First-seen value gets `1`.
    pub fn label_for(&mut self, category: Category, surface: &str) -> String {
        let key = (category, normalize(surface));
        if let Some(existing) = self.mapping.get(&key) {
            return existing.clone();
        }
        let n = self.counters.entry(category).or_insert(0);
        *n += 1;
        let label = format!("{} {}", category.label(), n);
        self.mapping.insert(key, label.clone());
        label
    }
}

/// Collapse whitespace and case so trivial variations map to the same pseudonym.
fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_surface_same_label() {
        let mut p = Pseudonymizer::new();
        let a = p.label_for(Category::Person, "Anna Svensson");
        let b = p.label_for(Category::Person, "anna   svensson");
        assert_eq!(a, b);
        assert_eq!(a, "Person 1");
    }

    #[test]
    fn distinct_values_increment() {
        let mut p = Pseudonymizer::new();
        assert_eq!(p.label_for(Category::Person, "Anna"), "Person 1");
        assert_eq!(p.label_for(Category::Person, "Erik"), "Person 2");
        assert_eq!(p.label_for(Category::Plats, "Lund"), "Plats 1");
    }
}

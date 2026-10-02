//! The edit list: what the user removed. Only intent is stored (item ids, manual ranges, a pause
//! limit); the actual cut times are recomputed from item times and the audio, so re-alignment or a
//! better cut-point rule never invalidates an edit.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Shortest piece of video worth keeping between two cuts (about one frame at 25 fps).
const MIN_KEEP: f64 = 0.04;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EditList {
    /// Struck items (words and sound blocks) by stable id.
    #[serde(default)]
    pub deleted: BTreeSet<u32>,
    /// Extra removed time ranges in source seconds (manual trims on the timeline).
    #[serde(default)]
    pub removed: Vec<(f64, f64)>,
    /// Pauses longer than this many seconds are shortened to it; `None` keeps pauses as they are.
    #[serde(default)]
    pub pause_limit: Option<f64>,
}

/// A transcript item on the source timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Item {
    pub id: u32,
    pub start: f64,
    pub end: f64,
}

/// Source ranges to keep, in order. `items` must be sorted by start; `pauses` are quiet stretches;
/// `cut(lo, hi)` picks the cut point between the end of one item and the start of the next.
pub fn keep_ranges(
    items: &[Item],
    edits: &EditList,
    pauses: &[(f64, f64)],
    duration: f64,
    cut: impl Fn(f64, f64) -> f64,
) -> Vec<(f64, f64)> {
    let mut removed: Vec<(f64, f64)> = Vec::new();
    let mut i = 0;
    while i < items.len() {
        if !edits.deleted.contains(&items[i].id) {
            i += 1;
            continue;
        }
        let first = i;
        while i + 1 < items.len() && edits.deleted.contains(&items[i + 1].id) {
            i += 1;
        }
        let start = if first == 0 { 0.0 } else { cut(items[first - 1].end, items[first].start) };
        let end = if i + 1 == items.len() { duration } else { cut(items[i].end, items[i + 1].start) };
        removed.push((start, end));
        i += 1;
    }
    removed.extend(edits.removed.iter().copied());
    if let Some(limit) = edits.pause_limit {
        // Keep `limit` seconds of each long pause, half on each side, so speech rhythm survives.
        for &(a, b) in pauses {
            if b - a > limit {
                removed.push((a + limit / 2.0, b - limit / 2.0));
            }
        }
    }
    removed.retain(|r| r.1 > r.0);
    removed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut keep = Vec::new();
    let mut t = 0.0;
    for (a, b) in removed {
        if a - t >= MIN_KEEP {
            keep.push((t, a.min(duration)));
        }
        t = t.max(b);
    }
    if duration - t >= MIN_KEEP {
        keep.push((t, duration));
    }
    keep
}

/// Map a source time to the edited timeline (None when it was cut away).
pub fn to_edited(keep: &[(f64, f64)], t: f64) -> Option<f64> {
    let mut offset = 0.0;
    for &(a, b) in keep {
        if t < a {
            return None;
        }
        if t <= b {
            return Some(offset + t - a);
        }
        offset += b - a;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<Item> {
        // Words at 1-2, 3-4, 5-6, 7-8 seconds.
        (0..4).map(|i| Item { id: i, start: 1.0 + 2.0 * i as f64, end: 2.0 + 2.0 * i as f64 }).collect()
    }
    fn mid(a: f64, b: f64) -> f64 {
        (a + b) / 2.0
    }

    #[test]
    fn nothing_deleted_keeps_everything() {
        assert_eq!(keep_ranges(&items(), &EditList::default(), &[], 10.0, mid), [(0.0, 10.0)]);
    }

    #[test]
    fn struck_runs_cut_between_neighbours() {
        let edits = EditList { deleted: [1, 2].into(), ..Default::default() };
        // Cut from between word 0/1 (2.5) to between word 2/3 (6.5).
        assert_eq!(keep_ranges(&items(), &edits, &[], 10.0, mid), [(0.0, 2.5), (6.5, 10.0)]);
    }

    #[test]
    fn struck_first_and_last_words_trim_the_ends() {
        let edits = EditList { deleted: [0, 3].into(), ..Default::default() };
        assert_eq!(keep_ranges(&items(), &edits, &[], 10.0, mid), [(2.5, 6.5)]);
    }

    #[test]
    fn manual_ranges_and_long_pauses_merge() {
        let edits = EditList { removed: vec![(2.2, 2.8)], pause_limit: Some(0.4), ..Default::default() };
        let pauses = [(2.0, 3.0), (4.0, 4.3)];
        // Pause 2-3 keeps 0.2 s each side -> remove 2.2-2.8 (same as the manual range); 4-4.3 is short.
        let keep = keep_ranges(&items(), &edits, &pauses, 10.0, mid);
        assert_eq!(keep.len(), 2);
        assert!((keep[0].1 - 2.2).abs() < 1e-9 && (keep[1].0 - 2.8).abs() < 1e-9);
    }

    #[test]
    fn source_to_edited_time() {
        let keep = [(0.0, 2.5), (6.5, 10.0)];
        assert_eq!(to_edited(&keep, 1.0), Some(1.0));
        assert_eq!(to_edited(&keep, 4.0), None);
        assert_eq!(to_edited(&keep, 7.0), Some(3.0));
    }
}

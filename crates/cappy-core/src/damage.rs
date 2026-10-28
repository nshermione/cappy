//! Collects the screen regions that must be repainted.

use cappy_layout::{Rect, Size};

/// Regions to repaint, in logical pixels.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Damage {
    #[default]
    None,
    Full,
    Regions(Vec<Rect>),
}

impl Damage {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Damaged rectangles, with [`Damage::Full`] expanded to the whole viewport.
    pub fn rects(&self, viewport: Size) -> Vec<Rect> {
        match self {
            Self::None => Vec::new(),
            Self::Full => vec![Rect::from_size(viewport)],
            Self::Regions(regions) => regions.clone(),
        }
    }

    /// Combines two pending damages.
    pub fn merge(self, other: Damage) -> Damage {
        match (self, other) {
            (Self::None, damage) | (damage, Self::None) => damage,
            (Self::Full, _) | (_, Self::Full) => Self::Full,
            (Self::Regions(mut first), Self::Regions(second)) => {
                first.extend(second);
                Self::Regions(first)
            }
        }
    }
}

/// Above this many separate regions the tracker repaints their bounding box instead.
const MAX_REGIONS: usize = 8;

/// Accumulates damaged rectangles and simplifies them when taken.
#[derive(Debug, Default)]
pub struct DamageTracker {
    full: bool,
    regions: Vec<Rect>,
}

impl DamageTracker {
    pub fn add(&mut self, rect: Rect) {
        if !self.full && !rect.is_empty() {
            self.regions.push(rect);
        }
    }

    pub fn add_full(&mut self) {
        self.full = true;
        self.regions.clear();
    }

    pub fn is_empty(&self) -> bool {
        !self.full && self.regions.is_empty()
    }

    /// The damage [`DamageTracker::take`] would return, without resetting.
    pub fn preview(&self, viewport: Size) -> Damage {
        Self {
            full: self.full,
            regions: self.regions.clone(),
        }
        .take(viewport)
    }

    /// Returns the simplified damage clipped to `viewport` and resets the tracker.
    pub fn take(&mut self, viewport: Size) -> Damage {
        let full = std::mem::take(&mut self.full);
        let regions = std::mem::take(&mut self.regions);
        if full {
            return Damage::Full;
        }
        let bounds = Rect::from_size(viewport);
        let clipped: Vec<Rect> = regions
            .iter()
            .filter_map(|rect| rect.intersection(&bounds))
            .collect();
        let merged = merge_touching(clipped);
        match merged.as_slice() {
            [] => Damage::None,
            [only] if only.contains_rect(&bounds) => Damage::Full,
            _ => Damage::Regions(merged),
        }
    }
}

/// Repeatedly unions rectangles that overlap or share an edge; collapses to one bounding box
/// when too many remain.
fn merge_touching(mut rects: Vec<Rect>) -> Vec<Rect> {
    let mut merged_any = true;
    while merged_any {
        merged_any = false;
        'search: for first in 0..rects.len() {
            for second in first + 1..rects.len() {
                if rects[first].touches(&rects[second]) {
                    let absorbed = rects.swap_remove(second);
                    rects[first] = rects[first].union(&absorbed);
                    merged_any = true;
                    break 'search;
                }
            }
        }
    }
    if rects.len() > MAX_REGIONS {
        let bounds = rects
            .iter()
            .skip(1)
            .fold(rects[0], |acc, rect| acc.union(rect));
        return vec![bounds];
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: Size = Size::new(100.0, 100.0);

    #[test]
    fn empty_tracker_reports_no_damage() {
        assert_eq!(DamageTracker::default().take(VIEWPORT), Damage::None);
    }

    #[test]
    fn disjoint_regions_stay_separate() {
        let mut tracker = DamageTracker::default();
        tracker.add(Rect::new(0.0, 0.0, 10.0, 10.0));
        tracker.add(Rect::new(50.0, 50.0, 10.0, 10.0));
        assert_eq!(
            tracker.take(VIEWPORT),
            Damage::Regions(vec![
                Rect::new(0.0, 0.0, 10.0, 10.0),
                Rect::new(50.0, 50.0, 10.0, 10.0)
            ])
        );
    }

    #[test]
    fn overlapping_and_adjacent_regions_merge() {
        let mut tracker = DamageTracker::default();
        tracker.add(Rect::new(0.0, 0.0, 10.0, 10.0));
        tracker.add(Rect::new(10.0, 0.0, 10.0, 10.0));
        tracker.add(Rect::new(15.0, 5.0, 10.0, 10.0));
        assert_eq!(
            tracker.take(VIEWPORT),
            Damage::Regions(vec![Rect::new(0.0, 0.0, 25.0, 15.0)])
        );
    }

    #[test]
    fn regions_are_clipped_to_the_viewport() {
        let mut tracker = DamageTracker::default();
        tracker.add(Rect::new(90.0, 90.0, 50.0, 50.0));
        tracker.add(Rect::new(200.0, 200.0, 5.0, 5.0));
        assert_eq!(
            tracker.take(VIEWPORT),
            Damage::Regions(vec![Rect::new(90.0, 90.0, 10.0, 10.0)])
        );
    }

    #[test]
    fn covering_the_viewport_becomes_full() {
        let mut tracker = DamageTracker::default();
        tracker.add(Rect::new(-5.0, -5.0, 200.0, 200.0));
        assert_eq!(tracker.take(VIEWPORT), Damage::Full);
    }

    #[test]
    fn too_many_regions_collapse_to_bounds() {
        let mut tracker = DamageTracker::default();
        for index in 0..=MAX_REGIONS {
            let offset = index as f32 * 10.0;
            tracker.add(Rect::new(offset, offset, 2.0, 2.0));
        }
        assert_eq!(
            tracker.take(VIEWPORT),
            Damage::Regions(vec![Rect::new(0.0, 0.0, 82.0, 82.0)])
        );
    }

    #[test]
    fn take_resets_the_tracker() {
        let mut tracker = DamageTracker::default();
        tracker.add_full();
        assert_eq!(tracker.take(VIEWPORT), Damage::Full);
        assert!(tracker.is_empty());
    }

    #[test]
    fn merging_pending_damage() {
        let region = Damage::Regions(vec![Rect::new(0.0, 0.0, 1.0, 1.0)]);
        assert_eq!(Damage::None.merge(region.clone()), region);
        assert_eq!(region.clone().merge(Damage::Full), Damage::Full);
        assert_eq!(
            region.clone().merge(region),
            Damage::Regions(vec![Rect::new(0.0, 0.0, 1.0, 1.0); 2])
        );
    }
}

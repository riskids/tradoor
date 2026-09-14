//! Which calendar period a partition file covers.
//!
//! History is split into CLOSED periods, so every file is immutable once written
//! and can be cached forever. The current month is kept as one file per closed
//! day; at month end the days are rolled into a month file, and for hourly and
//! daily bars months are rolled into a year at year end.

use tradoor_core::time::{civil, utc, DAY};
use tradoor_core::Interval;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Day,
    Month,
    Year,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Period {
    Year(i64),
    /// Month is 1–12.
    Month(i64, u32),
    Day(i64, u32, u32),
}

/// The coarsest file a base is rolled up into. Minutes stop at a month — a year
/// of them would be a large download to look at one afternoon — while hours and
/// days roll up to a year.
pub fn max_tier(base: Interval) -> Tier {
    if base == Interval::M1 { Tier::Month } else { Tier::Year }
}

impl Period {
    pub fn containing(ts: i64, tier: Tier) -> Self {
        let c = civil(ts);
        match tier {
            Tier::Year => Self::Year(c.year),
            Tier::Month => Self::Month(c.year, c.month0 + 1),
            Tier::Day => Self::Day(c.year, c.month0 + 1, c.day),
        }
    }

    pub fn tier(self) -> Tier {
        match self {
            Self::Year(_) => Tier::Year,
            Self::Month(..) => Tier::Month,
            Self::Day(..) => Tier::Day,
        }
    }

    pub fn start(self) -> i64 {
        match self {
            Self::Year(y) => utc(y, 0, 1),
            Self::Month(y, m) => utc(y, i64::from(m) - 1, 1),
            Self::Day(y, m, d) => utc(y, i64::from(m) - 1, d),
        }
    }

    /// Exclusive.
    pub fn end(self) -> i64 {
        match self {
            Self::Year(y) => utc(y + 1, 0, 1),
            Self::Month(y, m) => utc(y, i64::from(m), 1),
            Self::Day(..) => self.start() + DAY,
        }
    }

    pub fn is_closed(self, now: i64) -> bool {
        self.end() <= now
    }

    /// The file a closed bar is kept in as of `now`: the coarsest CLOSED period
    /// the base is allowed to roll up to.
    pub fn for_bar(base: Interval, open_time: i64, now: i64) -> Self {
        let max = max_tier(base);
        if max == Tier::Year {
            let year = Self::containing(open_time, Tier::Year);
            if year.is_closed(now) {
                return year;
            }
        }
        let month = Self::containing(open_time, Tier::Month);
        if month.is_closed(now) { month } else { Self::containing(open_time, Tier::Day) }
    }

    pub fn name(self) -> String {
        match self {
            Self::Year(y) => format!("{y:04}"),
            Self::Month(y, m) => format!("{y:04}-{m:02}"),
            Self::Day(y, m, d) => format!("{y:04}-{m:02}-{d:02}"),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('-').collect();
        let num = |p: &str, len: usize| (p.len() == len && p.bytes().all(|b| b.is_ascii_digit())).then(|| p.parse::<i64>().ok()).flatten();
        let period = match parts.as_slice() {
            [y] => Self::Year(num(y, 4)?),
            [y, m] => Self::Month(num(y, 4)?, u32::try_from(num(m, 2)?).ok()?),
            [y, m, d] => Self::Day(num(y, 4)?, u32::try_from(num(m, 2)?).ok()?, u32::try_from(num(d, 2)?).ok()?),
            _ => return None,
        };
        // Reject 2026-13 and 2026-02-30 by round-tripping through the calendar.
        (Self::containing(period.start(), period.tier()) == period).then_some(period)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tradoor_core::time::parse_iso;

    #[test]
    fn names_round_trip_and_reject_impossible_dates() {
        for p in [Period::Year(2026), Period::Month(2026, 2), Period::Day(2024, 2, 29)] {
            assert_eq!(Period::parse(&p.name()), Some(p));
        }
        assert_eq!(Period::parse("2026-13"), None);
        assert_eq!(Period::parse("2026-02-30"), None);
        assert_eq!(Period::parse("26-01"), None);
    }

    #[test]
    fn bars_go_to_the_coarsest_closed_period() {
        let now = parse_iso("2026-09-14T08:00Z").unwrap();
        let bar = |s| parse_iso(s).unwrap();
        assert_eq!(Period::for_bar(Interval::M1, bar("2025-03-02T10:00Z"), now), Period::Month(2025, 3));
        assert_eq!(Period::for_bar(Interval::H1, bar("2025-03-02T10:00Z"), now), Period::Year(2025));
        assert_eq!(Period::for_bar(Interval::H1, bar("2026-08-02T10:00Z"), now), Period::Month(2026, 8));
        assert_eq!(Period::for_bar(Interval::D1, bar("2026-09-13"), now), Period::Day(2026, 9, 13));
        assert_eq!(Period::Month(2026, 12).end(), bar("2027-01-01"));
    }
}

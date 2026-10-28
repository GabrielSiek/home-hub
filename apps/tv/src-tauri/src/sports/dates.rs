//! Calendar days without a date crate. ESPN files games under the US Eastern day, ge and
//! Lakers Brasil write Brasília time (UTC−3 all year: Brazil dropped daylight saving in 2019).

use serde::{Deserialize, Serialize};

use crate::photos::days_from_civil;

const DAY_MS: i64 = 86_400_000;
const HOUR_MS: i64 = 3_600_000;
/// Brasília.
const BRT_H: i64 = -3;
/// US Eastern standard time. A game at 00:15 EDT would land a day early, but no game starts then.
const ET_H: i64 = -5;

const MONTHS_PT: [&str; 12] = [
    "janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro",
    "novembro", "dezembro",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Day {
    pub y: i64,
    pub m: i64,
    pub d: i64,
}

impl Day {
    fn at(ms: i64, offset_h: i64) -> Day {
        civil_from_days((ms + offset_h * HOUR_MS).div_euclid(DAY_MS))
    }

    /// The Brasília calendar day at `ms`.
    pub fn brt(ms: i64) -> Day {
        Day::at(ms, BRT_H)
    }

    /// The day ESPN files a game starting at `ms` under.
    pub fn espn(ms: i64) -> Day {
        Day::at(ms, ET_H)
    }

    /// Today's slate at `now`: games after midnight in Brazil still belong to the evening before
    /// until the early morning, when the last NBA game is over.
    pub fn slate(now: i64) -> Day {
        Day::at(now - 3 * HOUR_MS, ET_H)
    }

    pub fn plus(self, days: i64) -> Day {
        civil_from_days(self.days() + days)
    }

    pub fn days(self) -> i64 {
        days_from_civil(self.y, self.m, self.d)
    }

    /// ESPN's `dates=` form: 20261026.
    pub fn compact(self) -> String {
        format!("{:04}{:02}{:02}", self.y, self.m, self.d)
    }

    /// 2026-10-26, as ge writes it.
    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }

    pub fn parse_iso(s: &str) -> Option<Day> {
        let mut p = s.get(..10)?.splitn(3, '-').map(|x| x.parse::<i64>().ok());
        let day = Day { y: p.next()??, m: p.next()??, d: p.next()?? };
        (1..=12).contains(&day.m).then_some(day)
    }

    /// "23 de outubro" (with the year given separately), as Lakers Brasil writes dates.
    pub fn parse_pt(text: &str, year: i64) -> Option<Day> {
        let lower = text.to_lowercase();
        let mut words = lower.split_whitespace();
        let d: i64 = words.next()?.parse().ok()?;
        if words.next()? != "de" {
            return None;
        }
        let month = words.next()?.trim_matches(|c: char| !c.is_alphabetic());
        let m = MONTHS_PT.iter().position(|n| *n == month)? as i64 + 1;
        Some(Day { y: year, m, d })
    }

    /// Unix ms of `h:m` Brasília time on this day.
    pub fn brt_ms(self, h: i64, m: i64) -> i64 {
        self.days() * DAY_MS + (h * 60 + m) * 60_000 - BRT_H * HOUR_MS
    }
}

/// The date `z` days after 1970-01-01 (H. Hinnant).
pub(crate) fn civil_from_days(z: i64) -> Day {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    Day { y, m, d }
}

/// "19:30:00" or "20h" or "22h30": hour and minute.
pub fn parse_hm(s: &str) -> Option<(i64, i64)> {
    let s = s.trim();
    let (h, m) = s.split_once([':', 'h']).unwrap_or((s, ""));
    let m = m.split(':').next().unwrap_or("");
    let h: i64 = h.trim().parse().ok()?;
    let m: i64 = if m.trim().is_empty() { 0 } else { m.trim().parse().ok()? };
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some((h, m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photos::rfc3339_ms;

    #[test]
    fn days_round_trip() {
        let day = Day { y: 2026, m: 10, d: 26 };
        assert_eq!(civil_from_days(day.days()), day);
        assert_eq!(Day { y: 2024, m: 2, d: 28 }.plus(1), Day { y: 2024, m: 2, d: 29 });
        assert_eq!(Day { y: 2026, m: 12, d: 31 }.plus(1), Day { y: 2027, m: 1, d: 1 });
        assert_eq!(day.compact(), "20261026");
        assert_eq!(Day::parse_iso("2026-10-26"), Some(day));
    }

    #[test]
    fn brasilia_and_espn_days() {
        // 21:30 in Brasília is 00:30 UTC the next day, still the 29th for both ESPN and ge.
        let kickoff = rfc3339_ms("2026-10-30T00:30Z").unwrap();
        assert_eq!(Day::brt(kickoff), Day { y: 2026, m: 10, d: 29 });
        assert_eq!(Day::espn(kickoff), Day { y: 2026, m: 10, d: 29 });
        assert_eq!(Day { y: 2026, m: 10, d: 29 }.brt_ms(21, 30), kickoff);
        // 01:00 in Brasília: still last night's NBA slate.
        let late = rfc3339_ms("2026-10-24T04:00Z").unwrap();
        assert_eq!(Day::slate(late), Day { y: 2026, m: 10, d: 23 });
        let morning = rfc3339_ms("2026-10-24T13:00Z").unwrap();
        assert_eq!(Day::slate(morning), Day { y: 2026, m: 10, d: 24 });
    }

    #[test]
    fn portuguese_dates_and_times() {
        assert_eq!(Day::parse_pt("23 de outubro", 2026), Some(Day { y: 2026, m: 10, d: 23 }));
        assert_eq!(Day::parse_pt("1 de março de 2027", 2027), Some(Day { y: 2027, m: 3, d: 1 }));
        assert_eq!(Day::parse_pt("sexta", 2026), None);
        assert_eq!(parse_hm("19:30:00"), Some((19, 30)));
        assert_eq!(parse_hm("20h"), Some((20, 0)));
        assert_eq!(parse_hm("22h30"), Some((22, 30)));
        assert_eq!(parse_hm("x"), None);
    }
}

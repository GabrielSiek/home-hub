//! Time without a date crate, in one place: unix milliseconds, calendar days, RFC 3339, and the
//! PC's local time from Windows' own zone rules. The old launcher spread these over photos,
//! sports, clock and calendar.

use std::sync::atomic::{AtomicI64, Ordering};

use serde::{Deserialize, Serialize};

pub const MIN: i64 = 60_000;
pub const HOUR: i64 = 60 * MIN;
pub const DAY: i64 = 24 * HOUR;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Where "now" comes from: the system clock, or one a test moves by hand.
pub trait Clock: Send + Sync + 'static {
    fn now_ms(&self) -> i64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        now_ms()
    }
}

/// A clock for tests.
pub struct FakeClock(AtomicI64);

impl FakeClock {
    pub fn new(ms: i64) -> Self {
        Self(AtomicI64::new(ms))
    }

    pub fn set(&self, ms: i64) {
        self.0.store(ms, Ordering::SeqCst);
    }

    pub fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

const MONTHS_PT: [&str; 12] = [
    "janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro",
    "novembro", "dezembro",
];

/// A calendar day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Day {
    pub y: i64,
    pub m: i64,
    pub d: i64,
}

impl Day {
    /// The day at `ms` in a zone `offset_ms` ahead of UTC.
    pub fn at(ms: i64, offset_ms: i64) -> Day {
        civil_from_days((ms + offset_ms).div_euclid(DAY))
    }

    pub fn plus(self, days: i64) -> Day {
        civil_from_days(self.days() + days)
    }

    /// Days since 1970-01-01.
    pub fn days(self) -> i64 {
        days_from_civil(self.y, self.m, self.d)
    }

    /// 20261026.
    pub fn compact(self) -> String {
        format!("{:04}{:02}{:02}", self.y, self.m, self.d)
    }

    /// 2026-10-26.
    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }

    pub fn parse_iso(s: &str) -> Option<Day> {
        let mut p = s.get(..10)?.splitn(3, '-').map(|x| x.parse::<i64>().ok());
        let day = Day { y: p.next()??, m: p.next()??, d: p.next()?? };
        (1..=12).contains(&day.m).then_some(day)
    }

    /// "23 de outubro" (with the year given separately).
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
}

/// The date `z` days after 1970-01-01 (H. Hinnant).
pub fn civil_from_days(z: i64) -> Day {
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

/// Days since 1970-01-01 for a date in the proleptic Gregorian calendar (H. Hinnant).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix ms from an RFC 3339 time such as `2024-05-01T12:34:56.789Z` or `…+02:00`.
pub fn rfc3339_ms(s: &str) -> Option<i64> {
    let (date, rest) = s.split_once(['T', 't', ' '])?;
    let mut d = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, mo, da) = (d.next()??, d.next()??, d.next()??);
    let zone_at = rest.find(['Z', 'z', '+', '-']).unwrap_or(rest.len());
    let (time, zone) = rest.split_at(zone_at);
    let mut t = time.splitn(3, ':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let sec: f64 = t.next().unwrap_or("0").parse().ok()?;
    let offset_min = match zone {
        "" | "Z" | "z" => 0,
        z => {
            let sign = if z.starts_with('-') { -1 } else { 1 };
            let (zh, zm) = z[1..].split_once(':')?;
            sign * (zh.parse::<i64>().ok()? * 60 + zm.parse::<i64>().ok()?)
        }
    };
    let secs = days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 - offset_min * 60;
    Some(secs * 1000 + (sec * 1000.0).round() as i64)
}

/// Unix ms as Google takes times: 2026-10-26T03:00:00Z.
pub fn rfc3339(ms: i64) -> String {
    let d = civil_from_days(ms.div_euclid(DAY));
    let s = ms.rem_euclid(DAY) / 1000;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", d.y, d.m, d.d, s / 3600, s / 60 % 60, s % 60)
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

/// The PC's local time: the system's zone rules give the offset from UTC at any moment (daylight
/// saving included); the rest is calendar arithmetic on "wall" times, the local date and time
/// written as if it were UTC.
pub mod local {
    use super::*;

    /// Milliseconds the local clock is ahead of UTC at `utc`.
    #[cfg(windows)]
    pub fn offset(utc: i64) -> i64 {
        use windows::Win32::Foundation::SYSTEMTIME;
        use windows::Win32::System::Time::SystemTimeToTzSpecificLocalTime;

        let d = civil_from_days(utc.div_euclid(DAY));
        let t = utc.rem_euclid(DAY);
        let st = SYSTEMTIME {
            wYear: d.y as u16,
            wMonth: d.m as u16,
            wDayOfWeek: 0,
            wDay: d.d as u16,
            wHour: (t / HOUR) as u16,
            wMinute: (t / MIN % 60) as u16,
            wSecond: (t / 1000 % 60) as u16,
            wMilliseconds: (t % 1000) as u16,
        };
        let mut local = SYSTEMTIME::default();
        // SAFETY: converts one SYSTEMTIME into another with the zone Windows is set to.
        if unsafe { SystemTimeToTzSpecificLocalTime(None, &st, &mut local) }.is_err() {
            return 0;
        }
        let wall = days_from_civil(i64::from(local.wYear), i64::from(local.wMonth), i64::from(local.wDay)) * DAY
            + i64::from(local.wHour) * HOUR
            + i64::from(local.wMinute) * MIN
            + i64::from(local.wSecond) * 1000
            + i64::from(local.wMilliseconds);
        wall - utc
    }

    /// Milliseconds the local clock is ahead of UTC at `utc`, by the system's zone (/etc/localtime
    /// or `TZ`, read once: a new zone takes a restart).
    #[cfg(not(windows))]
    pub fn offset(utc: i64) -> i64 {
        static ZONE: std::sync::LazyLock<jiff::tz::TimeZone> = std::sync::LazyLock::new(jiff::tz::TimeZone::system);
        jiff::Timestamp::from_millisecond(utc).map_or(0, |t| i64::from(ZONE.to_offset(t).seconds()) * 1000)
    }

    /// The local date and time at `utc`, as a wall time.
    pub fn wall(utc: i64) -> i64 {
        utc + offset(utc)
    }

    /// The moment a wall time happens. Twice over, so a time across a daylight saving change
    /// gets that day's offset.
    pub fn utc(wall: i64) -> i64 {
        let guess = wall - offset(wall);
        wall - offset(guess)
    }

    /// The local day at `utc`.
    pub fn day(utc: i64) -> Day {
        civil_from_days(wall(utc).div_euclid(DAY))
    }

    /// Monday 0 … Sunday 6, of a day.
    pub fn weekday(d: Day) -> u8 {
        // 1970-01-01 was a Thursday.
        (d.days() + 3).rem_euclid(7) as u8
    }

    /// "07:00".
    pub fn hm(utc: i64) -> String {
        let t = wall(utc).rem_euclid(DAY);
        format!("{:02}:{:02}", t / HOUR, t / MIN % 60)
    }

    /// The next time the clock reads `hour`:`minute` after `after`: on `date` only when given,
    /// else on one of `days` (Monday 0), else on whichever day comes first. None when `date` is
    /// past.
    pub fn next_at(hour: u8, minute: u8, days: &[u8], date: Option<Day>, after: i64) -> Option<i64> {
        let at = i64::from(hour) * HOUR + i64::from(minute) * MIN;
        if let Some(d) = date {
            return Some(utc(d.days() * DAY + at)).filter(|t| *t > after);
        }
        let today = day(after);
        (0..8).map(|k| today.plus(k)).find_map(|d| {
            let t = utc(d.days() * DAY + at);
            (t > after && (days.is_empty() || days.contains(&weekday(d)))).then_some(t)
        })
    }

    const WEEKDAYS_EN: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    const WEEKDAYS_PT: [&str; 7] =
        ["segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado", "domingo"];

    pub fn weekday_name(d: u8, pt: bool) -> &'static str {
        let i = usize::from(d.min(6));
        if pt {
            WEEKDAYS_PT[i]
        } else {
            WEEKDAYS_EN[i]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::local::*;
    use super::*;

    /// The workflow runs the tests in the house's zone. No daylight saving there since 2019.
    #[cfg(not(windows))]
    #[test]
    fn the_local_zone_is_the_systems() {
        if std::env::var("TZ").as_deref() == Ok("America/Sao_Paulo") {
            assert_eq!(offset(1_790_000_000_000), -3 * HOUR);
        }
    }

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
    fn portuguese_dates_and_times() {
        assert_eq!(Day::parse_pt("23 de outubro", 2026), Some(Day { y: 2026, m: 10, d: 23 }));
        assert_eq!(Day::parse_pt("1 de março de 2027", 2027), Some(Day { y: 2027, m: 3, d: 1 }));
        assert_eq!(Day::parse_pt("sexta", 2026), None);
        assert_eq!(parse_hm("19:30:00"), Some((19, 30)));
        assert_eq!(parse_hm("20h"), Some((20, 0)));
        assert_eq!(parse_hm("22h30"), Some((22, 30)));
        assert_eq!(parse_hm("x"), None);
    }

    #[test]
    fn rfc3339_times() {
        assert_eq!(rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_ms("2000-01-01T00:00:00Z"), Some(946_684_800_000));
        assert_eq!(rfc3339_ms("2000-01-01T02:00:00+02:00"), Some(946_684_800_000));
        assert_eq!(rfc3339_ms("1999-12-31T21:00:00.250-03:00"), Some(946_684_800_250));
        assert_eq!(rfc3339_ms("2024-02-29T12:00:00.123456789Z"), Some(1_709_208_000_123));
        assert_eq!(rfc3339_ms("not a time"), None);
        assert_eq!(rfc3339(946_684_800_000), "2000-01-01T00:00:00Z");
        assert_eq!(rfc3339_ms(&rfc3339(1_800_000_000_000)), Some(1_800_000_000_000));
    }

    #[test]
    fn weekdays() {
        // Monday, Oct 26 2026.
        assert_eq!(weekday(Day { y: 2026, m: 10, d: 26 }), 0);
        assert_eq!(weekday(Day { y: 2026, m: 11, d: 1 }), 6);
        assert_eq!(weekday(Day { y: 1970, m: 1, d: 1 }), 3);
    }

    #[test]
    fn next_alarm_times() {
        // Whatever the zone of the PC running the tests: a wall time maps back to itself.
        let now = utc(Day { y: 2026, m: 10, d: 26 }.days() * DAY + 6 * HOUR + 30 * MIN);
        assert_eq!(hm(now), "06:30");
        let seven = next_at(7, 0, &[], None, now).unwrap();
        assert_eq!((seven - now, hm(seven)), (30 * MIN, "07:00".to_string()));
        // Already past today: tomorrow.
        let six = next_at(6, 0, &[], None, now).unwrap();
        assert_eq!(day(six), Day { y: 2026, m: 10, d: 27 });
        // Weekends only, from a Monday: Saturday.
        let sat = next_at(9, 0, &[5, 6], None, now).unwrap();
        assert_eq!(day(sat), Day { y: 2026, m: 10, d: 31 });
        // A given day, and one that's gone.
        let on = next_at(7, 0, &[], Some(Day { y: 2026, m: 10, d: 28 }), now).unwrap();
        assert_eq!(day(on), Day { y: 2026, m: 10, d: 28 });
        assert_eq!(next_at(6, 0, &[], Some(Day { y: 2026, m: 10, d: 26 }), now), None);
    }

    #[test]
    fn a_fake_clock_moves_only_when_told() {
        let c = FakeClock::new(1_000);
        assert_eq!(c.now_ms(), 1_000);
        c.advance(MIN);
        assert_eq!(c.now_ms(), 1_000 + MIN);
        c.set(5);
        assert_eq!(c.now_ms(), 5);
    }
}

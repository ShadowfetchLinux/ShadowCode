//! Scheduled automations: a saved prompt that runs on a schedule as a normal
//! task in its own conversation. This module holds the plain rules: the
//! record, its validation, schedules (presets and five-field cron) and the
//! next-run and catch-up arithmetic. The database lives in
//! `store::automations`, the scheduler and runs in `engine::automations`,
//! the routes in `service::automations`.
//!
//! Times are Unix seconds (`f64`, like every other timestamp in the store).
//! A schedule is read in the computer's local time by default, or in UTC.
//! In local time a wall-clock time skipped by a daylight-saving change runs
//! at the first minute after the change, and a time that happens twice runs
//! only the first time.
use anyhow::{bail, ensure, Context, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Scheduler ticks are this far apart; a run this late still counts as on
/// time (never "missed"), even with a catch-up window of zero.
pub const ON_TIME_SLACK_SECS: f64 = 120.0;
/// How many missed times one "missed" history row counts at most.
const MISSED_COUNT_CAP: u32 = 1000;
/// Searching further than this for the next time means it never comes
/// (for example `0 0 30 2 *`, the 30th of February).
const SEARCH_DAYS: i64 = 366 * 5;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Schedule {
    /// Every hour at `minute` past.
    Hourly {
        #[serde(default)]
        minute: u8,
    },
    /// Every day at `time` (`HH:MM`).
    Daily { time: String },
    /// Monday to Friday at `time`.
    Weekdays { time: String },
    /// Once a week: `day` 0 = Sunday … 6 = Saturday.
    Weekly { day: u8, time: String },
    /// A five-field cron expression (minute hour day-of-month month
    /// day-of-week), or `@hourly`, `@daily`, `@weekly`, `@monthly`.
    Cron { expr: String },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Timezone {
    #[default]
    Local,
    Utc,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Checkout {
    /// A fresh managed worktree at the project's current commit.
    #[default]
    Worktree,
    /// The project folder itself (queued behind other work there).
    Main,
}

/// What happens when a run asks for approval (a shell command, or an edit
/// in a project set to ask first). Nobody may be watching, so the default is
/// to stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnApproval {
    #[default]
    Stop,
    /// Leave the request open for the user until the run's time limit.
    Wait,
}

/// A run's permissions never exceed the project's own; `read_only` lowers
/// them further for this automation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    #[default]
    Project,
    ReadOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub checkout: Checkout,
    pub permission: Permission,
    pub on_approval: OnApproval,
    pub max_runtime_minutes: u32,
    /// A time missed while ShadowCode was closed (or the computer asleep)
    /// runs once at startup when it is at most this old; older ones are
    /// recorded as missed. 0 never catches up.
    pub catch_up_minutes: u32,
    pub notify: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            checkout: Checkout::Worktree,
            permission: Permission::Project,
            on_approval: OnApproval::Stop,
            max_runtime_minutes: 60,
            catch_up_minutes: 120,
            notify: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Automation {
    pub id: String,
    pub workspace: PathBuf,
    pub name: String,
    pub prompt: String,
    /// A picker id; empty runs on the project's model.
    #[serde(default)]
    pub model: String,
    /// `code`, `plan` or `ask`.
    pub mode: String,
    pub schedule: Schedule,
    #[serde(default)]
    pub timezone: Timezone,
    #[serde(default)]
    pub options: Options,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub next_run_at: Option<f64>,
    #[serde(default)]
    pub created_at: f64,
    #[serde(default)]
    pub updated_at: f64,
}

/// The editable part of an automation (create and update requests).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Draft {
    pub name: String,
    pub prompt: String,
    pub model: String,
    pub mode: String,
    pub schedule: Option<Schedule>,
    pub timezone: Timezone,
    pub options: Options,
}

impl Draft {
    pub fn validate(&self) -> Result<()> {
        let name = self.name.trim();
        ensure!(
            !name.is_empty() && name.chars().count() <= 80,
            "Give the automation a name (up to 80 characters)"
        );
        ensure!(
            !name.chars().any(char::is_control),
            "The name cannot contain control characters"
        );
        ensure!(
            !self.prompt.trim().is_empty() && self.prompt.len() <= 32_000,
            "Write what the automation should do (up to 32000 bytes)"
        );
        ensure!(
            matches!(self.mode.as_str(), "code" | "plan" | "ask"),
            "Mode must be code, plan or ask"
        );
        ensure!(
            self.model.len() <= 512 && !self.model.chars().any(char::is_control),
            "Invalid model"
        );
        let schedule = self.schedule.as_ref().context("Choose a schedule")?;
        schedule.cron()?;
        ensure!(
            (1..=1440).contains(&self.options.max_runtime_minutes),
            "The time limit must be between 1 and 1440 minutes"
        );
        ensure!(
            self.options.catch_up_minutes <= 10_080,
            "The catch-up window is at most 10080 minutes (a week)"
        );
        Ok(())
    }
}

/// The engine's task mode for an automation mode (`ask` is the read-only
/// review mode).
pub fn task_mode(mode: &str) -> &'static str {
    match mode {
        "plan" => "plan",
        "ask" => "review",
        _ => "code",
    }
}

fn parse_time(text: &str) -> Result<(u32, u32)> {
    let (h, m) = text
        .trim()
        .split_once(':')
        .context("Use a time like 09:30")?;
    let (h, m): (u32, u32) = (
        h.parse().context("Use a time like 09:30")?,
        m.parse().context("Use a time like 09:30")?,
    );
    ensure!(h < 24 && m < 60, "Use a time between 00:00 and 23:59");
    Ok((h, m))
}

const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

impl Schedule {
    /// The equivalent cron expression, validated.
    pub fn cron(&self) -> Result<Cron> {
        let expr = match self {
            Schedule::Hourly { minute } => {
                ensure!(*minute < 60, "Minute must be between 0 and 59");
                format!("{minute} * * * *")
            }
            Schedule::Daily { time } => {
                let (h, m) = parse_time(time)?;
                format!("{m} {h} * * *")
            }
            Schedule::Weekdays { time } => {
                let (h, m) = parse_time(time)?;
                format!("{m} {h} * * 1-5")
            }
            Schedule::Weekly { day, time } => {
                ensure!(*day < 7, "Choose a day of the week");
                let (h, m) = parse_time(time)?;
                format!("{m} {h} * * {day}")
            }
            Schedule::Cron { expr } => expr.clone(),
        };
        Cron::parse(&expr)
    }
    /// Plain words for the list ("Weekdays at 09:00").
    pub fn describe(&self, timezone: Timezone) -> String {
        let text = match self {
            Schedule::Hourly { minute } => format!("Every hour at :{minute:02}"),
            Schedule::Daily { time } => format!("Every day at {}", time.trim()),
            Schedule::Weekdays { time } => format!("Weekdays at {}", time.trim()),
            Schedule::Weekly { day, time } => format!(
                "Every {} at {}",
                DAYS.get(*day as usize).unwrap_or(&"week"),
                time.trim()
            ),
            Schedule::Cron { expr } => format!("Custom schedule ({})", expr.trim()),
        };
        match timezone {
            Timezone::Local => text,
            Timezone::Utc => format!("{text} UTC"),
        }
    }
}

/// A parsed five-field cron expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cron {
    minutes: u64,
    hours: u32,
    days: u32,
    months: u16,
    weekdays: u8,
    /// Day-of-month / day-of-week were `*`: with both restricted, a day
    /// matches when either matches (classic cron).
    any_day: bool,
    any_weekday: bool,
}

fn names(field: usize) -> &'static [&'static str] {
    match field {
        3 => &[
            "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
        ],
        4 => &["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"],
        _ => &[],
    }
}

fn value(text: &str, field: usize, low: u32, high: u32) -> Result<u32> {
    let upper = text.to_ascii_uppercase();
    let value = if let Some(index) = names(field).iter().position(|n| *n == upper) {
        // Month names start at 1, weekday names at 0.
        index as u32 + if field == 3 { 1 } else { 0 }
    } else {
        text.parse::<u32>()
            .with_context(|| format!("'{text}' is not a number"))?
    };
    ensure!(
        (low..=high).contains(&value),
        "{value} is outside {low}–{high}"
    );
    Ok(value)
}

/// One field as a bit set; `true` when it was `*` (or `*/1`).
fn field(text: &str, field: usize, low: u32, high: u32) -> Result<(u64, bool)> {
    let mut bits = 0u64;
    let mut star = false;
    for part in text.split(',') {
        ensure!(!part.is_empty(), "Empty list item");
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => {
                let step: u32 = step
                    .parse()
                    .with_context(|| format!("'{step}' is not a step"))?;
                ensure!(step >= 1 && step <= high, "Step {step} is out of range");
                (range, step)
            }
            None => (part, 1),
        };
        let (start, end) = if range == "*" {
            if step == 1 {
                star = true;
            }
            (low, high)
        } else if let Some((a, b)) = range.split_once('-') {
            let (a, b) = (value(a, field, low, high)?, value(b, field, low, high)?);
            ensure!(a <= b, "Range {a}-{b} runs backwards");
            (a, b)
        } else {
            let a = value(range, field, low, high)?;
            // `5/15` means from 5 to the end in steps of 15.
            (a, if part.contains('/') { high } else { a })
        };
        let mut v = start;
        while v <= end {
            bits |= 1 << v;
            v += step;
        }
    }
    Ok((bits, star))
}

impl Cron {
    pub fn parse(expr: &str) -> Result<Self> {
        let expr = expr.trim();
        ensure!(expr.len() <= 200, "The schedule is too long");
        let expanded = match expr.to_ascii_lowercase().as_str() {
            "@hourly" => "0 * * * *".to_owned(),
            "@daily" | "@midnight" => "0 0 * * *".to_owned(),
            "@weekly" => "0 0 * * 0".to_owned(),
            "@monthly" => "0 0 1 * *".to_owned(),
            "@yearly" | "@annually" => "0 0 1 1 *".to_owned(),
            other if other.starts_with('@') => bail!("Unknown schedule shortcut {expr}"),
            _ => expr.to_owned(),
        };
        let fields: Vec<&str> = expanded.split_whitespace().collect();
        ensure!(
            fields.len() == 5,
            "A cron schedule has five fields: minute hour day month weekday"
        );
        let labels = ["minute", "hour", "day of month", "month", "day of week"];
        let ranges = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)];
        let mut parsed = [(0u64, false); 5];
        for (index, text) in fields.iter().enumerate() {
            let (low, high) = ranges[index];
            parsed[index] = field(text, index, low, high)
                .with_context(|| format!("Invalid {} field '{text}'", labels[index]))?;
        }
        let mut weekdays = parsed[4].0;
        // 7 is Sunday too.
        if weekdays & (1 << 7) != 0 {
            weekdays = (weekdays | 1) & !(1 << 7);
        }
        Ok(Self {
            minutes: parsed[0].0,
            hours: parsed[1].0 as u32,
            days: parsed[2].0 as u32,
            months: parsed[3].0 as u16,
            weekdays: weekdays as u8,
            any_day: parsed[2].1,
            any_weekday: parsed[4].1,
        })
    }

    fn day_matches(&self, date: NaiveDate) -> bool {
        let dom = self.days & (1 << date.day()) != 0;
        let dow = self.weekdays & (1 << date.weekday().num_days_from_sunday()) != 0;
        match (self.any_day, self.any_weekday) {
            (true, true) => true,
            (false, true) => dom,
            (true, false) => dow,
            (false, false) => dom || dow,
        }
    }

    /// The first scheduled time strictly after `after`, in `after`'s zone.
    pub fn next_after<Tz: TimeZone>(&self, after: &DateTime<Tz>) -> Option<DateTime<Tz>> {
        let zone = after.timezone();
        let local = after.naive_local();
        let mut t = local.with_second(0)?.with_nanosecond(0)? + Duration::minutes(1);
        let limit = t + Duration::days(SEARCH_DAYS);
        while t < limit {
            if self.months & (1 << t.month()) == 0 {
                let (y, m) = if t.month() == 12 {
                    (t.year() + 1, 1)
                } else {
                    (t.year(), t.month() + 1)
                };
                t = NaiveDate::from_ymd_opt(y, m, 1)?.and_hms_opt(0, 0, 0)?;
                continue;
            }
            if !self.day_matches(t.date()) {
                t = t.date().succ_opt()?.and_hms_opt(0, 0, 0)?;
                continue;
            }
            if self.hours & (1 << t.hour()) == 0 {
                t = t.date().and_hms_opt(t.hour(), 0, 0)? + Duration::hours(1);
                continue;
            }
            if self.minutes & (1 << t.minute()) == 0 {
                t += Duration::minutes(1);
                continue;
            }
            if let Some(found) = resolve(&zone, t) {
                if found > *after {
                    return Some(found);
                }
            }
            t += Duration::minutes(1);
        }
        None
    }
}

/// A wall-clock time in a zone: the only instant, the first of two, or the
/// first minute after a skipped stretch.
fn resolve<Tz: TimeZone>(zone: &Tz, wall: NaiveDateTime) -> Option<DateTime<Tz>> {
    if let Some(found) = zone.from_local_datetime(&wall).earliest() {
        return Some(found);
    }
    let mut probe = wall;
    for _ in 0..(26 * 60) {
        probe += Duration::minutes(1);
        if let Some(found) = zone.from_local_datetime(&probe).earliest() {
            return Some(found);
        }
    }
    None
}

fn instant(seconds: f64) -> DateTime<Utc> {
    Utc.timestamp_opt(seconds.floor() as i64, 0)
        .single()
        .unwrap_or_else(Utc::now)
}

/// The next scheduled time after `after` (Unix seconds), or `None` when the
/// schedule never runs again.
pub fn next_run(schedule: &Schedule, timezone: Timezone, after: f64) -> Result<Option<f64>> {
    let cron = schedule.cron()?;
    let after = instant(after);
    Ok(match timezone {
        Timezone::Utc => cron.next_after(&after).map(|t| t.timestamp() as f64),
        Timezone::Local => cron
            .next_after(&after.with_timezone(&chrono::Local))
            .map(|t| t.timestamp() as f64),
    })
}

/// Up to `count` upcoming times, for the editor's preview.
pub fn upcoming(
    schedule: &Schedule,
    timezone: Timezone,
    after: f64,
    count: usize,
) -> Result<Vec<f64>> {
    let mut out = Vec::new();
    let mut from = after;
    while out.len() < count {
        match next_run(schedule, timezone, from)? {
            Some(next) => {
                out.push(next);
                from = next;
            }
            None => break,
        }
    }
    Ok(out)
}

/// What the scheduler does with an automation whose time has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    NotYet,
    /// Run now; `catch_up` when it is later than a scheduler tick explains.
    Run {
        catch_up: bool,
    },
    /// Too late: record it as missed and wait for the next time.
    Missed,
}

pub fn decide(next_run_at: f64, now: f64, catch_up_minutes: u32) -> Due {
    if next_run_at > now {
        return Due::NotYet;
    }
    let late = now - next_run_at;
    if late <= ON_TIME_SLACK_SECS {
        Due::Run { catch_up: false }
    } else if late <= f64::from(catch_up_minutes) * 60.0 {
        Due::Run { catch_up: true }
    } else {
        Due::Missed
    }
}

/// How many scheduled times fell in `[first, now]` (at least 1, capped).
pub fn missed_count(schedule: &Schedule, timezone: Timezone, first: f64, now: f64) -> u32 {
    let mut count = 1;
    let mut from = first;
    while count < MISSED_COUNT_CAP {
        match next_run(schedule, timezone, from) {
            Ok(Some(next)) if next <= now => {
                count += 1;
                from = next;
            }
            _ => break,
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }
    fn next(expr: &str, after: &str) -> String {
        Cron::parse(expr)
            .unwrap()
            .next_after(&utc(after))
            .unwrap()
            .to_rfc3339()
    }

    #[test]
    fn cron_fields_ranges_steps_lists_and_names() {
        assert_eq!(
            next("*/15 * * * *", "2026-09-25T10:07:30Z"),
            "2026-09-25T10:15:00+00:00"
        );
        // Strictly after: exactly on a time moves to the following one.
        assert_eq!(
            next("*/15 * * * *", "2026-09-25T10:15:00Z"),
            "2026-09-25T10:30:00+00:00"
        );
        assert_eq!(
            next("0 9 * * 1-5", "2026-09-25T09:00:00Z"),
            "2026-09-28T09:00:00+00:00",
            "Friday 09:00 → Monday"
        );
        assert_eq!(
            next("30 8 * * MON,wed", "2026-09-25T00:00:00Z"),
            "2026-09-28T08:30:00+00:00"
        );
        assert_eq!(
            next("0 0 1 JAN *", "2026-09-25T00:00:00Z"),
            "2027-01-01T00:00:00+00:00"
        );
        assert_eq!(
            next("5/20 * * * *", "2026-09-25T10:26:00Z"),
            "2026-09-25T10:45:00+00:00"
        );
        // 7 is Sunday.
        assert_eq!(
            next("0 12 * * 7", "2026-09-25T00:00:00Z"),
            "2026-09-27T12:00:00+00:00"
        );
        // Day of month and weekday both restricted: either matches.
        assert_eq!(
            next("0 0 13 * 5", "2026-09-26T00:00:00Z"),
            "2026-10-02T00:00:00+00:00"
        );
        assert_eq!(
            next("@monthly", "2026-09-25T00:00:00Z"),
            "2026-10-01T00:00:00+00:00"
        );
        // Leap day.
        assert_eq!(
            next("0 0 29 2 *", "2026-09-25T00:00:00Z"),
            "2028-02-29T00:00:00+00:00"
        );
    }

    #[test]
    fn invalid_and_impossible_schedules_are_refused() {
        for bad in [
            "",
            "* * * *",
            "60 * * * *",
            "* 24 * * *",
            "* * 0 * *",
            "* * * 13 *",
            "* * * * 8",
            "*/0 * * * *",
            "5-1 * * * *",
            "a * * * *",
            "@sometimes",
            "1,,2 * * * *",
        ] {
            assert!(Cron::parse(bad).is_err(), "{bad:?} should be refused");
        }
        // Parses, but the 30th of February never comes.
        let never = Cron::parse("0 0 30 2 *").unwrap();
        assert!(never.next_after(&utc("2026-01-01T00:00:00Z")).is_none());
        assert!(Schedule::Daily {
            time: "25:00".into()
        }
        .cron()
        .is_err());
        assert!(Schedule::Weekly {
            day: 7,
            time: "09:00".into()
        }
        .cron()
        .is_err());
        assert!(Schedule::Hourly { minute: 60 }.cron().is_err());
    }

    #[test]
    fn presets_match_their_cron_and_describe_themselves() {
        let at = utc("2026-09-25T10:20:00Z").timestamp() as f64; // a Friday
        let t = |s: &Schedule| {
            let next = next_run(s, Timezone::Utc, at).unwrap().unwrap();
            DateTime::<Utc>::from_timestamp(next as i64, 0)
                .unwrap()
                .to_rfc3339()
        };
        let hourly = Schedule::Hourly { minute: 5 };
        assert_eq!(t(&hourly), "2026-09-25T11:05:00+00:00");
        let daily = Schedule::Daily {
            time: "09:30".into(),
        };
        assert_eq!(t(&daily), "2026-09-26T09:30:00+00:00");
        let weekdays = Schedule::Weekdays {
            time: "09:30".into(),
        };
        assert_eq!(t(&weekdays), "2026-09-28T09:30:00+00:00");
        let weekly = Schedule::Weekly {
            day: 3,
            time: "18:00".into(),
        };
        assert_eq!(t(&weekly), "2026-09-30T18:00:00+00:00");
        assert_eq!(hourly.describe(Timezone::Local), "Every hour at :05");
        assert_eq!(weekdays.describe(Timezone::Utc), "Weekdays at 09:30 UTC");
        assert_eq!(weekly.describe(Timezone::Local), "Every Wednesday at 18:00");
        let upcoming = upcoming(&daily, Timezone::Utc, at, 3).unwrap();
        assert_eq!(upcoming.len(), 3);
        assert_eq!(upcoming[1] - upcoming[0], 86_400.0);
    }

    /// Local time without daylight saving: a fixed offset stands in for the
    /// computer's zone, so the result does not depend on where tests run.
    #[test]
    fn local_times_follow_the_zone_offset() {
        let cron = Cron::parse("0 9 * * *").unwrap();
        let india = FixedOffset::east_opt(5 * 3600 + 1800).unwrap();
        let after = utc("2026-09-25T04:00:00Z").with_timezone(&india); // 09:30 local
        let found = cron.next_after(&after).unwrap();
        assert_eq!(found.to_rfc3339(), "2026-09-26T09:00:00+05:30");
        assert_eq!(
            found.with_timezone(&Utc).to_rfc3339(),
            "2026-09-26T03:30:00+00:00"
        );
        let west = FixedOffset::west_opt(7 * 3600).unwrap();
        let after = utc("2026-09-25T15:00:00Z").with_timezone(&west); // 08:00 local
        assert_eq!(
            cron.next_after(&after)
                .unwrap()
                .with_timezone(&Utc)
                .to_rfc3339(),
            "2026-09-25T16:00:00+00:00"
        );
        // The UTC reading of the same schedule differs by the offset.
        let at = utc("2026-09-25T04:00:00Z").timestamp() as f64;
        let in_utc = next_run(
            &Schedule::Daily {
                time: "09:00".into(),
            },
            Timezone::Utc,
            at,
        )
        .unwrap()
        .unwrap();
        assert_eq!(in_utc, utc("2026-09-25T09:00:00Z").timestamp() as f64);
        // The machine's own zone gives some future time within a day.
        let local = next_run(
            &Schedule::Daily {
                time: "09:00".into(),
            },
            Timezone::Local,
            at,
        )
        .unwrap()
        .unwrap();
        assert!(local > at && local <= at + 86_400.0 + 3600.0);
    }

    #[test]
    fn catch_up_and_missed_decisions() {
        let due = 1_000_000.0;
        assert_eq!(decide(due, due - 1.0, 60), Due::NotYet);
        assert_eq!(decide(due, due, 0), Due::Run { catch_up: false });
        // A scheduler tick's lateness is never "missed", even with no window.
        assert_eq!(decide(due, due + 90.0, 0), Due::Run { catch_up: false });
        assert_eq!(decide(due, due + 3_000.0, 60), Due::Run { catch_up: true });
        assert_eq!(decide(due, due + 3_700.0, 60), Due::Missed);
        assert_eq!(decide(due, due + 600.0, 0), Due::Missed);
        let hourly = Schedule::Hourly { minute: 0 };
        let first = utc("2026-09-25T01:00:00Z").timestamp() as f64;
        let now = utc("2026-09-25T05:30:00Z").timestamp() as f64;
        assert_eq!(missed_count(&hourly, Timezone::Utc, first, now), 5);
        assert_eq!(missed_count(&hourly, Timezone::Utc, first, first), 1);
    }

    #[test]
    fn drafts_are_validated() {
        let mut draft = Draft {
            name: "Nightly review".into(),
            prompt: "Review yesterday's commits".into(),
            mode: "ask".into(),
            schedule: Some(Schedule::Daily {
                time: "07:00".into(),
            }),
            ..Default::default()
        };
        draft.validate().unwrap();
        assert_eq!(task_mode("ask"), "review");
        draft.mode = "yolo".into();
        assert!(draft.validate().is_err());
        draft.mode = "code".into();
        draft.options.max_runtime_minutes = 0;
        assert!(draft.validate().is_err());
        draft.options.max_runtime_minutes = 30;
        draft.schedule = None;
        assert!(draft.validate().is_err());
        draft.schedule = Some(Schedule::Cron {
            expr: "0 0 * *".into(),
        });
        assert!(draft.validate().is_err());
        // Options default to the safe choices.
        let options: Options = serde_json::from_str("{}").unwrap();
        assert_eq!(options.checkout, Checkout::Worktree);
        assert_eq!(options.on_approval, OnApproval::Stop);
        assert_eq!(options.permission, Permission::Project);
    }
}

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    /// Tasks saved before this field existed were all weekly.
    #[default]
    Weekly,
    Once,
}

/// A task on a given weekday.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub title: String,
    /// Canonical "HH:MM", "" for no time, or legacy free text that could not be parsed.
    pub time: String,
    pub description: String,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub repeat: Repeat,
    /// ISO date of the last completion of a weekly task.
    #[serde(default)]
    pub completed_on: Option<String>,
    /// Keys written by other versions of the app, kept so a save does not drop them.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Task {
    pub fn is_done_this_week(&self) -> bool {
        self.completed_on
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            .is_some_and(|d| d.iso_week() == today().iso_week())
    }

    /// Order within a day: open before done, all-day, then by time, then untimed.
    fn sort_key(&self) -> (bool, u8, u32) {
        let slot = if self.all_day {
            (0, 0)
        } else if let Some((h, m)) = parse_time(&self.time) {
            (1, h * 60 + m)
        } else {
            (2, 0)
        };
        (self.is_done_this_week(), slot.0, slot.1)
    }
}

/// The editable fields of a task.
pub struct Draft {
    pub title: String,
    pub time: String,
    pub description: String,
    pub all_day: bool,
    pub repeat: Repeat,
}

/// All tasks, keyed by day index (0 = Monday .. 6 = Sunday).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeekData {
    pub days: [Vec<Task>; 7],
    #[serde(default)]
    pub next_id: u64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl WeekData {
    /// Load from disk. An unreadable file is copied aside rather than overwritten by the next save.
    pub fn load() -> Self {
        let path = Self::data_path();
        let Ok(raw) = fs::read_to_string(&path) else {
            return Self::default();
        };
        let mut data: Self = match serde_json::from_str(&raw) {
            Ok(data) => data,
            Err(err) => {
                let backup = path.with_extension(format!(
                    "json.broken-{}",
                    chrono::Local::now().format("%Y%m%d-%H%M%S")
                ));
                let _ = fs::copy(&path, &backup);
                eprintln!("weekplan: could not read {}: {err}; copied to {}", path.display(), backup.display());
                return Self::default();
            }
        };
        data.migrate();
        data
    }

    fn migrate(&mut self) {
        let mut max_id = None;
        for task in self.days.iter_mut().flatten() {
            max_id = max_id.max(Some(task.id));
            let time = task.time.trim();
            if time.eq_ignore_ascii_case("all day") {
                task.all_day = true;
                task.time.clear();
            } else if let Some((h, m)) = parse_time(time) {
                task.time = format!("{h:02}:{m:02}");
            }
        }
        if let Some(max_id) = max_id {
            self.next_id = self.next_id.max(max_id + 1);
        }
    }

    /// Write via a temporary file so a crash mid-write cannot truncate the plan.
    pub fn save(&self) {
        let path = Self::data_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let Ok(json) = serde_json::to_string_pretty(self) else {
            return;
        };
        let tmp = path.with_extension("json.tmp");
        if fs::write(&tmp, json).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }

    /// Tasks of a day in display order.
    pub fn sorted_day(&self, day: usize) -> Vec<Task> {
        let mut tasks = self.days[day].clone();
        tasks.sort_by_key(Task::sort_key);
        tasks
    }

    pub fn find(&self, id: u64) -> Option<(usize, &Task)> {
        self.days
            .iter()
            .enumerate()
            .find_map(|(day, tasks)| tasks.iter().find(|t| t.id == id).map(|t| (day, t)))
    }

    fn position(&self, id: u64) -> Option<(usize, usize)> {
        self.days
            .iter()
            .enumerate()
            .find_map(|(day, tasks)| tasks.iter().position(|t| t.id == id).map(|i| (day, i)))
    }

    pub fn add(&mut self, day: usize, draft: Draft) {
        let task = Task {
            id: self.next_id,
            title: draft.title,
            time: draft.time,
            description: draft.description,
            all_day: draft.all_day,
            repeat: draft.repeat,
            completed_on: None,
            extra: Map::new(),
        };
        self.next_id += 1;
        self.days[day].push(task);
        self.save();
    }

    /// Apply edits, moving the task to `day` if it changed. Returns the old day.
    pub fn update(&mut self, id: u64, day: usize, draft: Draft) -> Option<usize> {
        let old_day = self.move_to(id, day)?;
        let task = self.days[day].iter_mut().find(|t| t.id == id)?;
        task.title = draft.title;
        task.time = draft.time;
        task.description = draft.description;
        task.all_day = draft.all_day;
        task.repeat = draft.repeat;
        self.save();
        Some(old_day)
    }

    /// Move a task to the end of another day. Returns the old day.
    pub fn move_to(&mut self, id: u64, day: usize) -> Option<usize> {
        let (old_day, idx) = self.position(id)?;
        if old_day != day {
            let task = self.days[old_day].remove(idx);
            self.days[day].push(task);
            self.save();
        }
        Some(old_day)
    }

    /// Remove a task, returning where it was so it can be restored.
    pub fn remove(&mut self, id: u64) -> Option<(usize, usize, Task)> {
        let (day, idx) = self.position(id)?;
        let task = self.days[day].remove(idx);
        self.save();
        Some((day, idx, task))
    }

    pub fn restore(&mut self, day: usize, idx: usize, task: Task) {
        let idx = idx.min(self.days[day].len());
        self.days[day].insert(idx, task);
        self.save();
    }

    /// Mark a weekly task done or open for the current week.
    pub fn set_done(&mut self, id: u64, done: bool) {
        let Some((day, idx)) = self.position(id) else {
            return;
        };
        self.days[day][idx].completed_on = done.then(|| today().format("%Y-%m-%d").to_string());
        self.save();
    }

    fn data_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("weekplan")
            .join("tasks.json")
    }
}

/// Parse "14:30", "2:30 pm", "2pm", "9" and similar into (hour, minute).
pub fn parse_time(input: &str) -> Option<(u32, u32)> {
    let s = input.trim().to_ascii_lowercase().replace(' ', "").replace('.', ":");
    if s.is_empty() {
        return None;
    }
    let (clock, pm) = if let Some(c) = s.strip_suffix("pm") {
        (c, Some(true))
    } else if let Some(c) = s.strip_suffix("am") {
        (c, Some(false))
    } else {
        (s.as_str(), None)
    };
    let (h, m) = match clock.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None if clock.len() <= 2 => (clock.parse::<u32>().ok()?, 0),
        _ => return None,
    };
    let h = match pm {
        Some(pm) if (1..=12).contains(&h) => h % 12 + if pm { 12 } else { 0 },
        Some(_) => return None,
        None => h,
    };
    (h < 24 && m < 60).then_some((h, m))
}

pub const DAY_NAMES: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

/// Returns 0 for Monday .. 6 for Sunday.
pub fn today_index() -> usize {
    today().weekday().num_days_from_monday() as usize
}

/// Date of the given weekday in the current week.
pub fn date_of(day: usize) -> NaiveDate {
    let today = today();
    today - chrono::Days::new(today_index() as u64) + chrono::Days::new(day as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_times() {
        assert_eq!(parse_time("14:30"), Some((14, 30)));
        assert_eq!(parse_time("2:30 PM"), Some((14, 30)));
        assert_eq!(parse_time("12am"), Some((0, 0)));
        assert_eq!(parse_time("12 pm"), Some((12, 0)));
        assert_eq!(parse_time("9"), Some((9, 0)));
        assert_eq!(parse_time("9.15"), Some((9, 15)));
        assert_eq!(parse_time("25:00"), None);
        assert_eq!(parse_time("13pm"), None);
        assert_eq!(parse_time("All Day"), None);
    }

    #[test]
    fn migrates_saved_file_and_keeps_unknown_keys() {
        let raw = r#"{
            "days": [[], [
                {"id": 3, "title": "Work", "time": "All Day", "description": "x",
                 "completed_on": null, "priority": "normal", "tags": []},
                {"id": 17, "title": "Repair", "time": "6:00 PM", "description": ""}
            ], [], [], [], [], []],
            "next_id": 2,
            "templates": []
        }"#;
        let mut data: WeekData = serde_json::from_str(raw).unwrap();
        data.migrate();
        let work = &data.days[1][0];
        assert!(work.all_day && work.time.is_empty());
        assert_eq!(work.repeat, Repeat::Weekly);
        assert_eq!(data.days[1][1].time, "18:00");
        assert_eq!(data.next_id, 18);

        let out = serde_json::to_value(&data).unwrap();
        assert_eq!(out["templates"], serde_json::json!([]));
        assert_eq!(out["days"][1][0]["priority"], "normal");
    }

    fn task(id: u64, title: &str, time: &str, all_day: bool) -> Task {
        Task {
            id,
            title: title.into(),
            time: time.into(),
            description: String::new(),
            all_day,
            repeat: Repeat::Weekly,
            completed_on: None,
            extra: Map::new(),
        }
    }

    #[test]
    fn sorts_all_day_then_timed_then_untimed_then_done() {
        let mut data = WeekData::default();
        data.days[0] = vec![
            task(0, "untimed", "", false),
            task(1, "late", "18:00", false),
            task(2, "early", "08:00", false),
            task(3, "block", "", true),
        ];
        data.days[0][2].completed_on = Some(today().format("%Y-%m-%d").to_string());
        let order: Vec<_> = data.sorted_day(0).into_iter().map(|t| t.title).collect();
        assert_eq!(order, ["block", "late", "untimed", "early"]);
    }

    #[test]
    fn done_last_week_is_open_again() {
        let mut t = task(0, "gym", "", false);
        t.completed_on = Some((today() - chrono::Days::new(7)).format("%Y-%m-%d").to_string());
        assert!(!t.is_done_this_week());
    }
}

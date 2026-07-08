use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// A single recurring task for a given weekday.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub time: String,       // e.g. "6:00 PM" or "" if no time
    pub description: String, // optional extra detail
}

/// All tasks for a week, keyed by day index (0 = Monday .. 6 = Sunday).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeekData {
    pub days: [Vec<Task>; 7],
    #[serde(default)]
    pub next_id: u64,
}

impl WeekData {
    /// Load from disk, or create a fresh default.
    pub fn load() -> Self {
        let path = Self::data_path();
        if path.exists() {
            let raw = fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str(&raw).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Persist to disk.
    pub fn save(&self) {
        let path = Self::data_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(&path, json);
        }
    }

    /// Add a task to a specific day, returns the new task.
    pub fn add_task(&mut self, day: usize, title: String, time: String, description: String) -> Task {
        let task = Task {
            id: self.next_id,
            title,
            time,
            description,
        };
        self.next_id += 1;
        self.days[day].push(task.clone());
        self.save();
        task
    }

    /// Remove a task by id from a specific day.
    pub fn remove_task(&mut self, day: usize, task_id: u64) {
        self.days[day].retain(|t| t.id != task_id);
        self.save();
    }

    fn data_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("weekplan")
            .join("tasks.json")
    }
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

/// Returns 0 for Monday .. 6 for Sunday (ISO weekday - 1).
pub fn today_index() -> usize {
    use chrono::Datelike;
    let wd = chrono::Local::now().weekday().num_days_from_monday() as usize;
    wd // 0=Mon, 6=Sun
}

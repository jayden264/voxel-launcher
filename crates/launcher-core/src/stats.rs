//! Play-time sessions, kept in `stats.json` next to the settings.
//!
//! Only raw sessions are stored; the UI groups them by day (in the user's own
//! time zone), instance and so on.

use crate::config::{load_or_default, save_atomic, Dirs};
use crate::Result;
use serde::{Deserialize, Serialize};

pub const MAX_SESSIONS: usize = 5000;
/// Shorter than this is a crashed start or a mis-click, not play time.
const MIN_SECS: u64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub instance_id: String,
    /// Seconds since the epoch when the game window appeared.
    pub start: u64,
    pub secs: u64,
    /// The game ended with a non-zero exit code.
    pub crashed: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct StatsFile {
    version: u32,
    sessions: Vec<Session>,
}

/// Add a finished session. Blips shorter than a few seconds are dropped and
/// only the newest [`MAX_SESSIONS`] are kept.
pub fn record(dirs: &Dirs, session: Session) -> Result<()> {
    record_all(dirs, vec![session])
}

/// [`record`] for several sessions with a single write.
pub fn record_all(dirs: &Dirs, new: Vec<Session>) -> Result<()> {
    let new: Vec<Session> = new.into_iter().filter(|s| s.secs >= MIN_SECS).collect();
    if new.is_empty() {
        return Ok(());
    }
    let path = dirs.stats_file();
    let mut file: StatsFile = load_or_default(&path);
    file.version = 1;
    file.sessions.extend(new);
    if file.sessions.len() > MAX_SESSIONS {
        let extra = file.sessions.len() - MAX_SESSIONS;
        file.sessions.drain(..extra);
    }
    save_atomic(&path, &file)
}

/// Every recorded session, oldest first.
pub fn sessions(dirs: &Dirs) -> Vec<Session> {
    load_or_default::<StatsFile>(&dirs.stats_file()).sessions
}

//! The memo: one free-form markdown note written in the web editor,
//! persisted to disk, and shown on the device as its own screen between
//! dashboard frames.
//!
//! Storage is a single file (`$DATA_DIR/note.md`) so it survives restarts on
//! a PVC and stays readable/editable with plain tools. The edit timestamp is
//! the file's mtime — no sidecar metadata to drift out of sync.

use std::path::PathBuf;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Clone, Default)]
pub struct Note {
    pub markdown: String,
    /// `None` until the note has been written at least once.
    pub updated_at: Option<DateTime<Utc>>,
}

impl Note {
    /// Whitespace-only counts as empty: the editor leaves a stray newline
    /// behind when you clear it, and that shouldn't earn a whole screen.
    pub fn is_empty(&self) -> bool {
        self.markdown.trim().is_empty()
    }
}

pub struct NoteStore {
    path: PathBuf,
    note: RwLock<Note>,
}

impl NoteStore {
    /// Load the note from `$DATA_DIR/note.md` (default `./data`). A missing
    /// file is a fresh install, not an error.
    pub async fn open(data_dir: impl Into<PathBuf>) -> Self {
        let path = data_dir.into().join("note.md");
        let note = match tokio::fs::read_to_string(&path).await {
            Ok(markdown) => {
                let updated_at = tokio::fs::metadata(&path)
                    .await
                    .and_then(|m| m.modified())
                    .ok()
                    .map(DateTime::<Utc>::from);
                info!(
                    "loaded memo from {} ({} bytes)",
                    path.display(),
                    markdown.len()
                );
                Note {
                    markdown,
                    updated_at,
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Note::default(),
            Err(e) => {
                warn!("failed to read memo at {}: {e}", path.display());
                Note::default()
            }
        };
        Self {
            path,
            note: RwLock::new(note),
        }
    }

    pub async fn get(&self) -> Note {
        self.note.read().await.clone()
    }

    /// Persist and publish a new note. Written to a temp file and renamed
    /// over the old one, so a crash mid-write never leaves a torn note. The
    /// write lock is held across the disk write: the editor autosaves on
    /// every pause in typing, and two overlapping saves must not race on the
    /// temp file or land out of order.
    pub async fn set(&self, markdown: String) -> anyhow::Result<Note> {
        let mut guard = self.note.write().await;
        if let Some(dir) = self.path.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        let tmp = self.path.with_extension("md.tmp");
        tokio::fs::write(&tmp, &markdown).await?;
        tokio::fs::rename(&tmp, &self.path).await?;
        *guard = Note {
            markdown,
            updated_at: Some(SystemTime::now().into()),
        };
        Ok(guard.clone())
    }
}

// ── Playlist ────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Note,
}

/// Decides which screen the device gets on each poll. With a memo present
/// the device alternates dashboard ↔ memo; with none it always gets the
/// dashboard. A fresh edit jumps the queue so what you just wrote shows up
/// on the very next wake rather than one cycle later.
#[derive(Debug)]
pub struct Playlist {
    last: Screen,
    note_unseen: bool,
}

impl Default for Playlist {
    fn default() -> Self {
        Self {
            last: Screen::Dashboard,
            note_unseen: false,
        }
    }
}

impl Playlist {
    pub fn note_edited(&mut self) {
        self.note_unseen = true;
    }

    pub fn next(&mut self, has_note: bool) -> Screen {
        let screen = if !has_note {
            Screen::Dashboard
        } else if self.note_unseen || self.last == Screen::Dashboard {
            Screen::Note
        } else {
            Screen::Dashboard
        };
        if screen == Screen::Note {
            self.note_unseen = false;
        }
        self.last = screen;
        screen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_note_always_dashboard() {
        let mut p = Playlist::default();
        assert_eq!(p.next(false), Screen::Dashboard);
        assert_eq!(p.next(false), Screen::Dashboard);
    }

    #[test]
    fn note_alternates_with_dashboard() {
        let mut p = Playlist::default();
        assert_eq!(p.next(true), Screen::Note);
        assert_eq!(p.next(true), Screen::Dashboard);
        assert_eq!(p.next(true), Screen::Note);
    }

    #[test]
    fn edit_jumps_the_queue() {
        let mut p = Playlist::default();
        assert_eq!(p.next(true), Screen::Note);
        p.note_edited();
        assert_eq!(p.next(true), Screen::Note);
        assert_eq!(p.next(true), Screen::Dashboard);
    }

    #[tokio::test]
    async fn store_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("trmnl-note-{}", std::process::id()));
        let store = NoteStore::open(&dir).await;
        assert!(store.get().await.is_empty());
        store.set("# hi\n".into()).await.unwrap();
        let reopened = NoteStore::open(&dir).await;
        let note = reopened.get().await;
        assert_eq!(note.markdown, "# hi\n");
        assert!(note.updated_at.is_some());
        let _ = std::fs::remove_dir_all(dir);
    }
}

//! The handoff journal of one task, in the `log.md` file of its
//! directory.
//!
//! The journal is the **traversal state** of a task — what the last
//! session learned, what blocks, what comes next — as opposed to the
//! spec (authored content) and the plan (a checklist). Entries are
//! appended by the human and the agent, dated, and never rewritten:
//! the journal survives compactions and session changes because it
//! lives in the task directory, on the synchronized `tod` branch.
//!
//! Each entry is a markdown block:
//!
//! ```markdown
//! ## 2026-11-08T19:12:44Z - ndious
//! the entry text, one to a few lines
//! ```

use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};

/// Name of the journal file, inside the task directory.
pub const LOG_FILE: &str = "log.md";

/// One dated journal entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// When the entry was written (`2026-11-08T19:12:44Z`).
    pub timestamp: String,
    /// Who wrote it (`git user_name`, like the task author).
    pub author: String,
    /// The text of the entry, one to a few lines.
    pub text: String,
}

/// The journal of a single task. The entries are stored oldest
/// first, the file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Journal {
    pub entries: Vec<Entry>,
}

impl Journal {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Read the journal of a task. A missing file is an empty
    /// journal.
    pub fn read(task_dir: &Path) -> Result<Journal> {
        let path = task_dir.join(LOG_FILE);
        if !path.is_file() {
            return Ok(Journal::default());
        }
        let content =
            fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
        Ok(parse(&content))
    }

    /// The entries, most recent first — journals are read for what
    /// is happening now.
    pub fn recent(&self) -> Vec<&Entry> {
        self.entries.iter().rev().collect()
    }
}

/// Append a dated entry to the journal of a task, written now, by
/// 'author'. The file grows at the end: the journal is append-only.
pub fn append(task_dir: &Path, author: &str, text: &str) -> Result<Entry> {
    let text = text.trim();
    if text.is_empty() {
        bail!("cannot log an empty entry");
    }
    let entry = Entry {
        timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        author: author.to_string(),
        text: text.to_string(),
    };
    let path = task_dir.join(LOG_FILE);
    let mut existing = fs::read_to_string(&path).unwrap_or_default();
    if !existing.is_empty() {
        // a blank line between the blocks
        existing.push('\n');
    }
    existing.push_str(&format!(
        "## {} - {}\n{}\n",
        entry.timestamp, entry.author, entry.text
    ));
    fs::write(&path, existing).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(entry)
}

/// Parse the journal file: one block per `## ` heading, the text of
/// the entry on the following lines. Anything before the first
/// heading is ignored.
fn parse(content: &str) -> Journal {
    let mut entries = Vec::new();
    let mut current: Option<Entry> = None;
    for line in content.lines() {
        if let Some(header) = line.strip_prefix("## ") {
            if let Some(entry) = current.take() {
                entries.push(finalize(entry));
            }
            let (timestamp, author) = header
                .split_once(" - ")
                .map(|(t, a)| (t.trim().to_string(), a.trim().to_string()))
                .unwrap_or((header.to_string(), "unknown".to_string()));
            current = Some(Entry {
                timestamp,
                author,
                text: String::new(),
            });
        } else if let Some(entry) = current.as_mut() {
            entry.text.push_str(line);
            entry.text.push('\n');
        }
    }
    if let Some(entry) = current {
        entries.push(finalize(entry));
    }
    Journal { entries }
}

/// Trim the trailing newlines the line accumulation added.
fn finalize(mut entry: Entry) -> Entry {
    entry.text = entry.text.trim_end().to_string();
    entry
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "## {} - {}\n{}", self.timestamp, self.author, self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_an_empty_journal() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::read(dir.path()).unwrap();
        assert!(journal.is_empty());
        assert!(journal.recent().is_empty());
    }

    #[test]
    fn append_then_read_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let first = append(dir.path(), "tester", "first entry\nspans two lines").unwrap();
        let second = append(dir.path(), "tester", "second entry").unwrap();

        let journal = Journal::read(dir.path()).unwrap();
        assert_eq!(journal.entries.len(), 2);
        // the file order is chronological...
        assert_eq!(journal.entries[0].text, "first entry\nspans two lines");
        assert_eq!(journal.entries[1].text, "second entry");
        // ...the reading order is most recent first
        let recent = journal.recent();
        assert_eq!(recent[0].text, "second entry");
        assert_eq!(recent[1].text, "first entry\nspans two lines");
        // dated, with the author
        assert_eq!(second.author, "tester");
        assert_eq!(second.timestamp.len(), "2026-11-08T19:12:44Z".len());

        // the file itself: one block per entry, a blank line between
        let raw = std::fs::read_to_string(dir.path().join(LOG_FILE)).unwrap();
        assert!(raw.starts_with("## "), "{raw}");
        assert!(raw.matches("\n## ").count() + 1 == 2, "{raw}");
        assert!(raw.contains("\n\n## "), "a blank line separates: {raw}");
        let _ = first;
    }

    #[test]
    fn text_is_trimmed_and_empty_entries_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), "tester", "  padded text  \n\n").unwrap();
        assert_eq!(
            Journal::read(dir.path()).unwrap().entries[0].text,
            "padded text"
        );
        assert!(append(dir.path(), "tester", "   ").is_err());
        assert!(append(dir.path(), "tester", "").is_err());
        // nothing broken by the refused append
        assert_eq!(Journal::read(dir.path()).unwrap().entries.len(), 1);
    }

    #[test]
    fn unparsable_headers_degrade_to_unknown_author() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(LOG_FILE),
            "## no author separator here\ntext without a dash\n",
        )
        .unwrap();
        let journal = Journal::read(dir.path()).unwrap();
        assert_eq!(journal.entries.len(), 1);
        assert_eq!(journal.entries[0].author, "unknown");
        assert_eq!(journal.entries[0].timestamp, "no author separator here");
        // content before the first heading is ignored
        std::fs::write(
            dir.path().join(LOG_FILE),
            "stray line\n## 2026-11-08T19:12:44Z - tester\nthe entry\n",
        )
        .unwrap();
        assert_eq!(Journal::read(dir.path()).unwrap().entries.len(), 1);
    }

    #[test]
    fn entry_display_prints_the_block() {
        let entry = Entry {
            timestamp: "2026-11-08T19:12:44Z".to_string(),
            author: "tester".to_string(),
            text: "the text".to_string(),
        };
        assert_eq!(
            entry.to_string(),
            "## 2026-11-08T19:12:44Z - tester\nthe text"
        );
    }
}

//! The todo task: a dot document with a fixed schema, one file per task.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::schema::{Document, Field, Schema};
use crate::types::FieldType;

/// The task schema, ported from tod-cli's storage.js.
pub fn task_schema() -> Schema {
    Schema::new(vec![
        ("description", FieldType::Text),
        ("author", FieldType::Text),
        ("created", FieldType::Iso8601),
        ("updated", FieldType::Iso8601),
        ("done", FieldType::Bool),
        ("rfr", FieldType::Bool),
        ("tags", FieldType::Text),
        ("content", FieldType::Text),
    ])
}

/// Normalize a tag list: lowercase, trimmed, without duplicates
/// or empties, in their order of appearance.
pub fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim().to_lowercase();
        if !tag.is_empty() && !out.contains(&tag) {
            out.push(tag);
        }
    }
    out
}

/// Tags are one text field: comma separated.
fn parse_tags(value: &str) -> Vec<String> {
    normalize_tags(value.split(',').map(str::to_string).collect())
}

fn stringify_tags(tags: &[String]) -> String {
    tags.join(", ")
}

/// A single todo. One task = one dot file named by its uid (sha1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub uid: String,
    pub description: String,
    pub author: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub done: bool,
    /// Ready for review: the work is finished, waiting for a
    /// review before `tod finish`.
    #[serde(default)]
    pub rfr: bool,
    /// Free-form labels, normalized lowercase.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl Task {
    /// Render the task as a dot document file (uid is the file name,
    /// not part of the content, exactly like tod-cli).
    pub fn stringify(&self) -> String {
        task_schema().stringify(&self.to_document())
    }

    /// Parse a task file. 'uid' comes from the file name.
    pub fn parse(uid: &str, content: &str) -> Result<Task> {
        let doc = task_schema()
            .parse(content)
            .with_context(|| format!("invalid task file for uid '{uid}'"))?;

        let description = doc
            .text("description")
            .context("missing Description field")?;
        let author = doc.text("author").unwrap_or_else(|| "unknown".to_string());
        let created = match doc.get("created") {
            Some(Field::Datetime(dt)) => *dt,
            _ => bail!("missing or invalid Created field"),
        };
        let updated = match doc.get("updated") {
            Some(Field::Datetime(dt)) => *dt,
            _ => created,
        };
        let done = match doc.get("done") {
            Some(Field::Bool(b)) => *b,
            _ => false,
        };
        let rfr = match doc.get("rfr") {
            Some(Field::Bool(b)) => *b,
            _ => false,
        };
        let tags = doc.text("tags").as_deref().map(parse_tags).unwrap_or_default();
        let content = doc.text("content");

        Ok(Task {
            uid: uid.to_string(),
            description,
            author,
            created,
            updated,
            done,
            rfr,
            tags,
            content,
        })
    }

    fn to_document(&self) -> Document {
        let mut doc = Document::default();
        doc.set("description", Field::Text(self.description.clone()));
        doc.set("author", Field::Text(self.author.clone()));
        doc.set("created", Field::Datetime(self.created));
        doc.set("updated", Field::Datetime(self.updated));
        doc.set("done", Field::Bool(self.done));
        doc.set("rfr", Field::Bool(self.rfr));
        if !self.tags.is_empty() {
            doc.set("tags", Field::Text(stringify_tags(&self.tags)));
        }
        if let Some(content) = &self.content {
            let lines: Vec<String> =
                content.split('\n').map(|l| l.to_string()).collect();
            doc.set("content", Field::Block(lines));
        }
        doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Task {
        let dt = DateTime::parse_from_rfc3339("2026-10-06T18:12:44Z")
            .unwrap()
            .with_timezone(&Utc);
        Task {
            uid: "abc123".to_string(),
            description: "write the tests".to_string(),
            author: "tester".to_string(),
            created: dt,
            updated: dt,
            done: false,
            rfr: false,
            tags: Vec::new(),
            content: None,
        }
    }

    #[test]
    fn task_file_round_trip() {
        let task = sample();
        let file = task.stringify();
        let back = Task::parse("abc123", &file).unwrap();
        assert_eq!(back, task);
    }

    #[test]
    fn task_file_looks_like_the_original_format() {
        let file = sample().stringify();
        assert!(file.starts_with("Description: write the tests\nAuthor: tester\n"));
        assert!(file.contains("Created: 2026-10-06 18:12:44 +0000\n"));
        assert!(file.contains("Done: 0\n"));
    }

    #[test]
    fn tags_are_normalized_and_round_trip() {
        let mut task = sample();
        task.tags = normalize_tags(vec![
            "Rust".into(),
            " CLI ".into(),
            "rust".into(),
            "".into(),
            "Git".into(),
        ]);
        assert_eq!(task.tags, vec!["rust".to_string(), "cli".to_string(), "git".to_string()]);

        let back = Task::parse("abc123", &task.stringify()).unwrap();
        assert_eq!(back.tags, task.tags);

        // a file without tags reads as an empty list
        let bare = sample();
        assert!(bare.tags.is_empty());
    }

    #[test]
    fn multi_line_content_round_trip() {
        let mut task = sample();
        task.content = Some("first line\nsecond line".to_string());
        let back = Task::parse("abc123", &task.stringify()).unwrap();
        assert_eq!(back.content, Some("first line\nsecond line".to_string()));
    }

    #[test]
    fn missing_description_is_an_error() {
        assert!(Task::parse("x", "Author: tester\n").is_err());
    }

    #[test]
    fn missing_dates_are_rejected() {
        assert!(Task::parse("x", "Description: hi\n").is_err());
    }
}

//! The todo task: a dot document with a fixed schema, one file per task.
//!
//! A task lives in a directory named by its uid (a sha1 hash), inside
//! the directory of its state (backlog, todo, review, test, done,
//! canceled). The directory holds `task.tod` (this document), and
//! optionally `plan.md` (its micro-todo list) and `spec.md` (the
//! fine-grained work description handed to a development agent).

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::schema::{Document, Field, Schema};
use crate::types::FieldType;

/// The task file inside the task directory.
pub const TASK_FILE: &str = "task.tod";

/// The task schema, ported from tod-cli's storage.js. The state of a
/// task is its directory, not a field; the working branch is a field
/// since the per-context directories are gone.
pub fn task_schema() -> Schema {
    Schema::new(vec![
        ("description", FieldType::Text),
        ("author", FieldType::Text),
        ("created", FieldType::Iso8601),
        ("updated", FieldType::Iso8601),
        ("branch", FieldType::Text),
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

/// A single todo. One task = one directory named by its uid (sha1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub uid: String,
    pub description: String,
    pub author: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    /// The working branch of the task, set by `tod start`.
    #[serde(default)]
    pub branch: Option<String>,
    /// Free-form labels, normalized lowercase.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl Task {
    /// Render the task as a dot document file (uid is the directory
    /// name, not part of the content).
    pub fn stringify(&self) -> String {
        task_schema().stringify(&self.to_document())
    }

    /// Parse a task file. 'uid' comes from the directory name.
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
        let branch = doc.text("branch").filter(|b| !b.trim().is_empty());
        let tags = doc.text("tags").as_deref().map(parse_tags).unwrap_or_default();
        let content = doc.text("content");

        Ok(Task {
            uid: uid.to_string(),
            description,
            author,
            created,
            updated,
            branch,
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
        if let Some(branch) = &self.branch {
            doc.set("branch", Field::Text(branch.clone()));
        }
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

//! The dot document format: schema-based parse and stringify.
//!
//! A dot document is a plain-text file: one "Key: value" entry per
//! line, multi-line values indented with a tab. Keys are stored in
//! camelCase and rendered in Title Case, exactly like tod-cli.

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};

use crate::types::{FieldType, parse_bool, parse_datetime, stringify_bool, stringify_datetime};

/// A parsed field value.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Text(String),
    Bool(bool),
    Datetime(DateTime<Utc>),
    Block(Vec<String>),
}

/// A parsed dot document: ordered key/field entries.
#[derive(Debug, Clone, Default)]
pub struct Document {
    entries: Vec<(String, Field)>,
}

impl Document {
    pub fn get(&self, key: &str) -> Option<&Field> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn set(&mut self, key: &str, value: Field) {
        if let Some(entry) = self.entries.iter_mut().find(|(k, _)| k == key) {
            entry.1 = value;
        } else {
            self.entries.push((key.to_string(), value));
        }
    }

    pub fn entries(&self) -> &[(String, Field)] {
        &self.entries
    }

    /// Text of a field: single line, or block joined with newlines.
    pub fn text(&self, key: &str) -> Option<String> {
        match self.get(key) {
            Some(Field::Text(s)) => Some(s.clone()),
            Some(Field::Block(lines)) => Some(lines.join("\n")),
            _ => None,
        }
    }
}

/// camelCase keys, as stored in documents ("Test String" -> "testString").
pub fn parse_key(key: &str) -> String {
    key.split(' ')
        .enumerate()
        .map(|(i, word)| if i == 0 { word.to_lowercase() } else { word.to_string() })
        .collect()
}

/// Title Case keys, as rendered in files ("testLongString" -> "Test Long String").
pub fn stringify_key(key: &str) -> String {
    let spaced: String = key
        .chars()
        .enumerate()
        .flat_map(|(i, c)| {
            if i > 0 && c.is_uppercase() {
                vec![' ', c]
            } else {
                vec![c]
            }
        })
        .collect();
    spaced
        .split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A raw (untyped) entry as read from a file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RawValue {
    Line(String),
    Block(Vec<String>),
}

fn parse_raw(content: &str) -> Vec<(String, RawValue)> {
    let mut entries: Vec<(String, RawValue)> = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('\t') {
            // multi-line value: append to the previous entry
            if let Some(entry) = entries.last_mut() {
                match &mut entry.1 {
                    RawValue::Block(lines) => lines.push(rest.to_string()),
                    RawValue::Line(text) => {
                        let first = std::mem::take(text);
                        if first.is_empty() {
                            entry.1 = RawValue::Block(vec![rest.to_string()]);
                        } else {
                            entry.1 = RawValue::Block(vec![first, rest.to_string()]);
                        }
                    }
                }
            }
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            entries.push((parse_key(key.trim()), RawValue::Line(value.trim().to_string())));
        }
    }
    entries
}

/// A document schema: known fields and their types. Unknown keys are
/// kept as plain text, like in the original lib.
#[derive(Debug, Clone)]
pub struct Schema {
    fields: Vec<(&'static str, FieldType)>,
}

impl Schema {
    pub fn new(fields: Vec<(&'static str, FieldType)>) -> Self {
        Schema { fields }
    }

    fn field_type(&self, key: &str) -> Option<FieldType> {
        self.fields.iter().find(|(k, _)| *k == key).map(|(_, t)| *t)
    }

    /// Parse a dot document into typed fields.
    pub fn parse(&self, content: &str) -> Result<Document> {
        let mut doc = Document::default();
        for (key, raw) in parse_raw(content) {
            let field = match self.field_type(&key) {
                Some(FieldType::Bool) => match raw {
                    RawValue::Line(v) => Field::Bool(parse_bool(&v)),
                    _ => bail!("field '{key}' must be a single line"),
                },
                Some(FieldType::Iso8601) => match raw {
                    RawValue::Line(v) => Field::Datetime(parse_datetime(&v)?),
                    _ => bail!("field '{key}' must be a single line"),
                },
                Some(FieldType::Text) | None => match raw {
                    RawValue::Line(v) => Field::Text(v),
                    RawValue::Block(lines) => Field::Block(lines),
                },
            };
            doc.set(&key, field);
        }
        Ok(doc)
    }

    /// Render a document back to the dot file format: schema fields
    /// first (in declaration order), then any extra keys.
    pub fn stringify(&self, doc: &Document) -> String {
        let mut keys: Vec<String> = self
            .fields
            .iter()
            .map(|(k, _)| k.to_string())
            .collect();
        for (key, _) in doc.entries() {
            if !keys.contains(key) {
                keys.push(key.clone());
            }
        }

        let mut out = String::new();
        for key in keys {
            let Some(field) = doc.get(&key) else { continue };
            let rendered = match field {
                Field::Text(s) => s.clone(),
                Field::Bool(b) => stringify_bool(*b).to_string(),
                Field::Datetime(dt) => stringify_datetime(dt),
                Field::Block(lines) => format!("\n\t{}", lines.join("\n\t")),
            };
            let line = format!("{}: {}", stringify_key(&key), rendered);
            if out.is_empty() {
                out.push_str(line.trim_start_matches('\n'));
            } else {
                out.push('\n');
                out.push_str(&line);
            }
        }
        out.push('\n');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task_schema() -> Schema {
        Schema::new(vec![
            ("description", FieldType::Text),
            ("done", FieldType::Bool),
            ("created", FieldType::Iso8601),
        ])
    }

    // Ports of the original lib's own tests.

    #[test]
    fn parse_key_converts_file_keys() {
        assert_eq!(parse_key("Test"), "test");
        assert_eq!(parse_key("Test String"), "testString");
        assert_eq!(parse_key("Test Long String"), "testLongString");
    }

    #[test]
    fn stringify_key_converts_back() {
        assert_eq!(stringify_key("testLongString"), "Test Long String");
        assert_eq!(stringify_key("test"), "Test");
        assert_eq!(parse_key(&stringify_key("testLongString")), "testLongString");
    }

    #[test]
    fn parse_reads_all_field_types() {
        let doc = task_schema()
            .parse(
                "Description: hello world\nDone: 1\nCreated: 2026-10-06 18:12:44 +0000\nExtra: kept as text\n",
            )
            .unwrap();
        assert_eq!(doc.get("description"), Some(&Field::Text("hello world".into())));
        assert_eq!(doc.get("done"), Some(&Field::Bool(true)));
        assert!(matches!(doc.get("created"), Some(Field::Datetime(_))));
        assert_eq!(doc.get("extra"), Some(&Field::Text("kept as text".into())));
    }

    #[test]
    fn parse_reads_multi_line_blocks() {
        let content = "Description: first\n\tsecond\n\tthird\n";
        let doc = task_schema().parse(content).unwrap();
        assert_eq!(
            doc.get("description"),
            Some(&Field::Block(vec!["first".into(), "second".into(), "third".into()]))
        );
        assert_eq!(doc.text("description").unwrap(), "first\nsecond\nthird");
    }

    #[test]
    fn stringify_orders_schema_fields_first() {
        let mut doc = Document::default();
        doc.set("done", Field::Bool(false));
        doc.set("description", Field::Text("hello".into()));
        let out = task_schema().stringify(&doc);
        assert!(out.starts_with("Description: hello\nDone: 0"));
    }

    #[test]
    fn stringify_renders_blocks_with_tabs() {
        let mut doc = Document::default();
        doc.set("description", Field::Block(vec!["one".into(), "two".into()]));
        let out = task_schema().stringify(&doc);
        assert!(out.contains("Description: \n\tone\n\ttwo\n"));
    }

    #[test]
    fn parse_stringify_round_trip() {
        let content = "Description: hello\nDone: 1\nCreated: 2026-10-06 18:12:44 +0000\n";
        let schema = task_schema();
        let doc = schema.parse(content).unwrap();
        assert_eq!(schema.stringify(&doc), content);
    }

    #[test]
    fn invalid_datetime_in_typed_field_is_an_error() {
        let err = task_schema().parse("Created: nope\n").is_err();
        assert!(err);
    }
}

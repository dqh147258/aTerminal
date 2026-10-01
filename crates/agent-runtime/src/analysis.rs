//! Resolve references to complete, visible original lines before strict observation validation.
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};

const MAX_LINES: usize = 256;
const MAX_TABLE_BYTES: usize = 16 * 1024;

#[derive(Serialize)]
struct NumberedLine {
    line_number: usize,
    text: String,
}

#[derive(Serialize)]
pub(crate) struct LineTable {
    record_id: String,
    lines: Vec<NumberedLine>,
    truncated: bool,
}

impl LineTable {
    pub(crate) fn empty(record_id: &str) -> Self {
        Self {
            record_id: record_id.into(),
            lines: vec![],
            truncated: false,
        }
    }

    pub(crate) fn visible(
        record_id: &str,
        original: &str,
        visible: &str,
        offset: Option<usize>,
    ) -> Self {
        let mut table = Self::empty(record_id);
        if visible.is_empty() {
            return table;
        }
        let offset = match offset {
            Some(offset) => offset,
            None if original == visible => 0,
            None => {
                let Some(offset) = original.find(visible) else {
                    return table;
                };
                let next = offset
                    + visible
                        .chars()
                        .next()
                        .expect("nonempty visible body")
                        .len_utf8();
                if original[next..].contains(visible) {
                    return table;
                }
                offset
            }
        };
        let Some(end) = offset.checked_add(visible.len()) else {
            return table;
        };
        if original.get(offset..end) != Some(visible) {
            return table;
        }
        let mut position = 0;
        let complete = original
            .split('\n')
            .enumerate()
            .filter_map(|(index, text)| {
                let start = position;
                position += text.len() + 1;
                (start >= offset
                    && start + text.len() <= end
                    && (start < end || end == original.len()))
                .then_some((index + 1, text))
            })
            .collect::<Vec<_>>();
        // Prefer the most recent visible lines, including TUI rows, without truncating a line.
        let mut bytes = serde_json::to_vec(&table)
            .expect("line table serializes")
            .len();
        for (line_number, text) in complete.into_iter().rev() {
            if table.lines.len() == MAX_LINES {
                table.truncated = true;
                break;
            }
            if text.len() > MAX_TABLE_BYTES {
                table.truncated = true;
                continue;
            }
            let line = NumberedLine {
                line_number,
                text: text.into(),
            };
            let size = serde_json::to_vec(&line)
                .expect("original line serializes")
                .len()
                + 1;
            if bytes + size > MAX_TABLE_BYTES {
                table.truncated = true;
                continue;
            }
            bytes += size;
            table.lines.push(line);
        }
        table.lines.reverse();
        table
    }

    fn reference(&self, value: &Value) -> Result<Vec<String>> {
        let object = value
            .as_object()
            .context("invalid_analysis_line_reference")?;
        ensure!(
            !object.contains_key("text"),
            "ambiguous_analysis_line_reference"
        );
        ensure!(
            object.len() == 3
                && object.contains_key("record_id")
                && object.contains_key("line_start")
                && object.contains_key("line_end"),
            "invalid_analysis_line_reference"
        );
        ensure!(
            value["record_id"] == self.record_id,
            "analysis_line_reference_record_mismatch"
        );
        let start = value["line_start"]
            .as_u64()
            .context("invalid_analysis_line_reference")?;
        let end = value["line_end"]
            .as_u64()
            .context("invalid_analysis_line_reference")?;
        ensure!(
            start > 0 && end >= start && end - start < MAX_LINES as u64,
            "invalid_analysis_line_reference"
        );
        (start..=end)
            .map(|number| {
                self.lines
                    .iter()
                    .find(|line| line.line_number as u64 == number)
                    .map(|line| line.text.clone())
                    .context("analysis_line_reference_out_of_view")
            })
            .collect()
    }

    fn expand_text(&self, value: &mut Value) -> Result<()> {
        if value.get("line_start").is_some() || value.get("line_end").is_some() {
            let lines = self.reference(value)?;
            *value = json!({"record_id":self.record_id,"text":lines.join("\n")});
        }
        Ok(())
    }

    pub(crate) fn expand(&self, mut digest: Value) -> Result<Value> {
        if let Some(quotes) = digest.get_mut("key_quotes").and_then(Value::as_array_mut) {
            for quote in quotes {
                self.expand_text(quote)?;
            }
        }
        if let Some(facts) = digest.get_mut("facts").and_then(Value::as_array_mut) {
            for fact in facts {
                if let Some(evidence) = fact.get_mut("evidence") {
                    self.expand_text(evidence)?;
                }
            }
        }
        if let Some(lines) = digest.get_mut("tui_lines").and_then(Value::as_array_mut) {
            let mut expanded = Vec::new();
            for line in lines.iter() {
                if line.is_object() {
                    // A TUI range can include blank layout separators. They cannot form
                    // search anchors; validate the entire range before omitting those rows.
                    expanded.extend(
                        self.reference(line)?
                            .into_iter()
                            .filter(|text| !text.trim().is_empty())
                            .map(Value::String),
                    );
                } else {
                    expanded.push(line.clone());
                }
                ensure!(expanded.len() <= MAX_LINES, "tui_annotation_limit");
            }
            *lines = expanded;
        }
        Ok(digest)
    }
}

#[cfg(test)]
pub(crate) const HAIR_SPACE_UPDATE: &str = "│ ✨\u{200a}Update available! 0.159.2 -> 0.159.3                                                             │";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Scope, Store};

    fn observation() -> (tempfile::TempDir, Store, Scope, String, String) {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("data/history.db")).unwrap();
        let scope = store.agent("owner", "desktop", Some("session")).unwrap();
        let user = store
            .accept_user_authorized(&scope, "u", "inspect", json!({}), None, false)
            .unwrap();
        let body = [
            "stable log",
            HAIR_SPACE_UPDATE,
            "  Shall we turn “huh?” into “aha!”?",
            "› Ask Codex to do anything",
            "  GPT-6-Astra high · Context 100% left · 0 in · 0 out · Fast off",
        ]
        .join("\n");
        let record = store
            .archive(
                &scope,
                &user.run_id,
                "observation",
                "text",
                json!({"alternate_screen":false,"head_lines":2,"tail_lines":2}),
                body.as_bytes(),
            )
            .unwrap();
        (temp, store, scope, record.id, body)
    }

    #[test]
    fn original_line_references_preserve_real_hair_space_and_tui_unicode() {
        let (_temp, store, scope, record, body) = observation();
        let table = LineTable::visible(&record, &body, &body, None);
        let digest = table.expand(json!({
            "summary":"Original TUI inspected",
            "key_quotes":[{"record_id":record,"line_start":2,"line_end":3}],
            "facts":[{"claim":"Update notice is visible","evidence":{"record_id":record,"line_start":2,"line_end":2},"certainty":"observed"}],
            "tui_lines":[{"record_id":record,"line_start":2,"line_end":5}]
        })).unwrap();
        let summary = store.analyze(&scope, &record, digest).unwrap();
        assert_eq!(summary["facts"][0]["evidence"]["text"], HAIR_SPACE_UPDATE);
        assert_eq!(
            summary["key_quotes"][0]["text"],
            format!("{HAIR_SPACE_UPDATE}\n  Shall we turn “huh?” into “aha!”?")
        );
        assert_eq!(summary["tui_lines"].as_array().unwrap().len(), 4);
        for line in body.split('\n').skip(1) {
            assert!(
                summary["tui_lines"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(line))
            );
        }
        assert_eq!(
            summary["search_tail_anchor"]["lines"],
            json!(["stable log"])
        );
        assert_eq!(
            store.record_bytes(&scope, &record).unwrap().1,
            body.as_bytes()
        );
    }

    #[test]
    fn legacy_text_remains_exact_and_unicode_variants_still_fail() {
        let (_temp, store, scope, record, body) = observation();
        let table = LineTable::visible(&record, &body, &body, None);
        let digest = json!({"summary":"Legacy text","key_quotes":[{"record_id":record,"text":HAIR_SPACE_UPDATE}],"facts":[{"claim":"Prompt visible","evidence":"› Ask Codex to do anything","certainty":"observed"}],"tui_lines":[HAIR_SPACE_UPDATE,"› Ask Codex to do anything"]});
        assert_eq!(table.expand(digest.clone()).unwrap(), digest);
        store.analyze(&scope, &record, digest).unwrap();
        let changed = HAIR_SPACE_UPDATE.replace('\u{200a}', " ");
        let bad_quote = table.expand(json!({"summary":"Invalid quote","key_quotes":[{"record_id":record,"text":changed}],"tui_lines":[]})).unwrap();
        assert_eq!(
            store
                .analyze(&scope, &record, bad_quote)
                .unwrap_err()
                .to_string(),
            "invalid_analysis_quote"
        );
        let bad_fact = table.expand(json!({"summary":"Invalid fact","facts":[{"claim":"Update visible","evidence":{"record_id":record,"text":changed},"certainty":"observed"}],"tui_lines":[]})).unwrap();
        assert_eq!(
            store
                .analyze(&scope, &record, bad_fact)
                .unwrap_err()
                .to_string(),
            "unverified_analysis_evidence"
        );
    }

    #[test]
    fn tui_reference_ranges_ignore_blank_layout_rows_without_loosening_validation() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("data/history.db")).unwrap();
        let scope = store.agent("owner", "desktop", Some("session")).unwrap();
        let user = store
            .accept_user_authorized(&scope, "u", "inspect", json!({}), None, false)
            .unwrap();
        let body = "• Working (38s • esc to interrupt)\n  └ Tip: Use /keymap to configure keyboard shortcuts.\n\n\n› Ask Codex to do anything\n\n  GPT-6-Astra high · Context 100% left\n  ← for agents · ? for shortcuts";
        let record = store
            .archive(
                &scope,
                &user.run_id,
                "read",
                "text",
                json!({"alternate_screen":false}),
                body.as_bytes(),
            )
            .unwrap();
        let table = LineTable::visible(&record.id, body, body, None);
        let digest = table.expand(json!({"summary":"Working TUI","tui_lines":[{"record_id":record.id,"line_start":1,"line_end":8}]})).unwrap();
        let summary = store.analyze(&scope, &record.id, digest).unwrap();
        assert_eq!(summary["tui_lines"].as_array().unwrap().len(), 5);
        assert!(
            summary["tui_lines"]
                .as_array()
                .unwrap()
                .iter()
                .all(|line| !line.as_str().unwrap().trim().is_empty())
        );
        assert!(
            table
                .expand(json!({"tui_lines":[{"record_id":record.id,"line_start":1,"line_end":9}]}))
                .is_err()
        );
        let legacy = table
            .expand(json!({"summary":"Invalid legacy blank","tui_lines":[""]}))
            .unwrap();
        assert_eq!(
            store
                .analyze(&scope, &record.id, legacy)
                .unwrap_err()
                .to_string(),
            "invalid_analysis_tui_line"
        );
        assert_eq!(
            store.record_bytes(&scope, &record.id).unwrap().1,
            body.as_bytes()
        );
    }

    #[test]
    fn wrong_out_of_view_incomplete_and_mixed_references_are_rejected() {
        let table = LineTable::visible("record", "one\ntwo", "one\ntwo", None);
        let bad = [
            json!({"record_id":"other","line_start":1,"line_end":1}),
            json!({"record_id":"record","line_start":0,"line_end":1}),
            json!({"record_id":"record","line_start":2,"line_end":1}),
            json!({"record_id":"record","line_start":1,"line_end":3}),
            json!({"record_id":"record","line_start":1.0,"line_end":1}),
            json!({"record_id":"record","line_start":-1,"line_end":1}),
            json!({"record_id":"record","line_start":"1","line_end":1}),
            json!({"record_id":"record","line_start":1}),
            json!({"record_id":"record","line_end":1}),
            json!({"record_id":"record","line_start":1,"line_end":1,"text":null}),
            json!({"record_id":"record","line_start":1,"line_end":1,"text":"one"}),
            json!({"record_id":"record","line_start":1,"line_end":1,"extra":true}),
            json!({"record_id":"record","line_start":1,"line_end":u64::MAX}),
        ];
        for reference in bad {
            for digest in [
                json!({"key_quotes":[reference]}),
                json!({"facts":[{"claim":"unknown","certainty":"unknown","evidence":reference}]}),
                json!({"tui_lines":[reference]}),
            ] {
                assert!(
                    table.expand(digest).is_err(),
                    "reference must reject: {reference}"
                );
            }
        }
    }

    #[test]
    fn partial_pages_only_reference_complete_visible_original_lines() {
        assert!(
            LineTable::visible("r", "a\na\na", "a\na", None)
                .lines
                .is_empty()
        );
        let original = format!(
            "hidden-before\npartial-start\n{HAIR_SPACE_UPDATE}\n› Ask Codex to do anything\npartial-end\nhidden-after"
        );
        let start = original.find("partial-start").unwrap() + 3;
        let end = original.find("partial-end").unwrap() + 5;
        let table = LineTable::visible("r", &original, &original[start..end], Some(start));
        assert_eq!(
            table
                .lines
                .iter()
                .map(|line| line.line_number)
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
        assert_eq!(
            table
                .reference(&json!({"record_id":"r","line_start":3,"line_end":4}))
                .unwrap(),
            vec![
                HAIR_SPACE_UPDATE.to_owned(),
                "› Ask Codex to do anything".into()
            ]
        );
        for number in [1, 2, 5, 6] {
            assert!(
                table
                    .reference(&json!({"record_id":"r","line_start":number,"line_end":number}))
                    .is_err()
            );
        }
        assert!(
            table
                .reference(&json!({"record_id":"r","line_start":2,"line_end":4}))
                .is_err()
        );
        assert!(
            LineTable::visible("r", "large incomplete row", "incomplete", None)
                .lines
                .is_empty()
        );
        assert!(
            LineTable::visible("r", "row\nrow\n", "row", None)
                .lines
                .is_empty()
        );
        assert!(
            LineTable::visible("r", original.as_str(), "not original", Some(0))
                .lines
                .is_empty()
        );
        assert_eq!(
            LineTable::visible("r", "row\nrow\n", "row", Some(4)).lines[0].line_number,
            2
        );
    }

    #[test]
    fn tables_are_bounded_and_missing_binary_or_omitted_lines_cannot_be_referenced() {
        let original = (1..=600)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let table = LineTable::visible("r", &original, &original, None);
        assert!(table.truncated);
        assert_eq!(table.lines.len(), MAX_LINES);
        assert_eq!(table.lines.last().unwrap().line_number, 600);
        assert!(
            table
                .reference(&json!({"record_id":"r","line_start":1,"line_end":1}))
                .is_err()
        );
        let escaped = [
            "\t".repeat(3000),
            "\"".repeat(3000),
            "\\".repeat(3000),
            "tail".into(),
        ]
        .join("\n");
        let table = LineTable::visible("r", &escaped, &escaped, None);
        assert!(serde_json::to_vec(&table).unwrap().len() <= MAX_TABLE_BYTES);
        assert!(table.truncated);
        let large = format!("small\n{}\ntail", "x".repeat(MAX_TABLE_BYTES));
        let table = LineTable::visible("r", &large, &large, None);
        assert!(
            table
                .reference(&json!({"record_id":"r","line_start":1,"line_end":3}))
                .is_err()
        );
        let binary = LineTable::empty("binary");
        assert!(
            binary
                .expand(json!({"key_quotes":[{"record_id":"binary","line_start":1,"line_end":1}]}))
                .is_err()
        );
        for digest in [Value::Null, json!("string"), json!([]), json!(1)] {
            assert_eq!(binary.expand(digest.clone()).unwrap(), digest);
        }
    }
}

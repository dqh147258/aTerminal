//! Long replies must survive both the event ledger and the recovery checkpoint.
use ai_terminal_agent_runtime::store::{Scope, Store};
use serde_json::{Value, json};

fn original(store: &Store, scope: &Scope, record: &str) -> Value {
    let mut cursor = None;
    let mut body = String::new();
    loop {
        let page = store
            .record_page(scope, record, "body", cursor.as_deref(), 12288)
            .unwrap();
        assert_eq!(page["kind"], "history_event");
        let part = page["body"].as_str().unwrap();
        assert!(part.len() <= 12288);
        body.push_str(part);
        cursor = page["cursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            return serde_json::from_str(&body).unwrap();
        }
    }
}

#[test]
fn large_unicode_replies_are_paged_without_losing_text_or_interruption_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data/messages.db");
    let store = Store::open(&path).unwrap();
    let scope = store.agent("owner", "desktop", Some("terminal")).unwrap();
    let text = "长回复🙂\n\"quoted\"\\path ".repeat(4000);
    assert!(text.len() > 24 * 1024);
    for (kind, event_id) in [("assistant", "answer"), ("interaction", "interrupted")] {
        let value = if kind == "interaction" {
            json!({"text":text,"source":"model_interrupted","state":"interrupted",
                "interrupted":true,"error":"model_stream_interrupted","run_id":"run"})
        } else {
            json!({"text":text})
        };
        store
            .append_identified(&scope, kind, "root", event_id, value, None)
            .unwrap();
    }
    let escaped = "\u{0000}\n\"\\🙂".repeat(4000);
    store
        .append_identified(
            &scope,
            "interaction",
            "root",
            "escaped",
            json!({
                "text":escaped,"source":"model_interrupted","interrupted":true,
                "error":"\u{0000}".repeat(4000)
            }),
            None,
        )
        .unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    let page = store.history(&scope, None).unwrap();
    for item in page.items {
        assert!(serde_json::to_vec(&item.value).unwrap().len() < 12 * 1024);
        assert_eq!(item.value["partial"], true);
        let record = item.value["record_id"].as_str().unwrap();
        let full = original(&store, &scope, record);
        assert_eq!(
            full["text"].as_str(),
            Some(if item.id == "escaped" {
                escaped.as_str()
            } else {
                text.as_str()
            })
        );
        if item.id == "interrupted" {
            assert_eq!(item.value["interrupted"], true);
            assert_eq!(item.value["source"], "model_interrupted");
            assert_eq!(item.value["state"], "interrupted");
            assert_eq!(full["interrupted"], true);
            assert!(full.get("partial").is_none());
        }
        let foreign = store
            .agent("another-owner", "desktop", Some("terminal"))
            .unwrap();
        assert!(
            store
                .record_page(&foreign, record, "body", None, 12288)
                .is_err()
        );
    }
}

#[test]
fn megabyte_reply_and_checkpoint_survive_reopening_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data/messages.db");
    let store = Store::open(&path).unwrap();
    let scope = store.agent("owner", "desktop", Some("terminal")).unwrap();
    let text = "完整答案🙂\n".repeat(100_000);
    assert!(text.len() > 1024 * 1024);
    store
        .append_identified(
            &scope,
            "assistant",
            "root",
            "large-answer",
            json!({"text":text}),
            None,
        )
        .unwrap();
    let checkpoint =
        json!({"projection":{"entries":[{"id":"large-answer","text":text}]},"pending":[]});
    store.checkpoint(&scope, 7, 42, &checkpoint).unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.projection(&scope).unwrap(), Some((7, 42, checkpoint)));
    let page = store.history(&scope, None).unwrap();
    let record = page.items[0].value["record_id"].as_str().unwrap();
    assert_eq!(original(&store, &scope, record)["text"], text);
}

#[test]
fn message_expansion_keeps_control_limits_and_rejects_oversized_replacements_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("data/messages.db")).unwrap();
    let scope = store.agent("owner", "desktop", Some("terminal")).unwrap();
    let initial = json!({"projection":"last valid checkpoint"});
    store.checkpoint(&scope, 1, 1, &initial).unwrap();
    let oversized = json!({"text":"x".repeat(17 * 1024 * 1024)});
    assert_eq!(
        store
            .checkpoint(&scope, 2, 2, &oversized)
            .unwrap_err()
            .to_string(),
        "storage_item_limit"
    );
    assert_eq!(store.projection(&scope).unwrap(), Some((1, 1, initial)));
    for (kind, size) in [("assistant", 3 * 1024 * 1024), ("agent_report", 32 * 1024)] {
        assert_eq!(
            store
                .append(&scope, kind, Some("root"), json!({"text":"x".repeat(size)}))
                .unwrap_err()
                .to_string(),
            "storage_item_limit"
        );
    }
    assert!(store.history(&scope, None).unwrap().items.is_empty());
}

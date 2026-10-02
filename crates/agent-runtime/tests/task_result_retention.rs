use ai_terminal_agent_runtime::{
    host::{AgentHost, Budget, ToolContext},
    store::{Retention, Store},
};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[test]
fn oversized_error_preserves_terminal_state_and_releases_child_pins() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("data/tasks.db");
    let store = Store::open(&path).unwrap();
    let global = store.agent("owner", "desktop", None).unwrap();
    let child = store.agent("owner", "desktop", Some("session")).unwrap();
    let root = store
        .accept_user(&global, "root", "coordinate", json!({}))
        .unwrap();
    let task = store
        .delegate(
            &child,
            &root.root_user_message_id,
            "task",
            "work",
            json!({}),
            None,
        )
        .unwrap();
    // Both multibyte boundaries and JSON escaping must remain inside the report limit.
    let error = "错误\n\u{0000}\"".repeat(6000);
    store
        .finish_run_with_error(&child, &task.run_id, "failed", Some(&error))
        .unwrap();
    let outcome = store.agent_task(&global, &task.run_id, 1024).unwrap().1;
    assert_eq!(outcome["state"], "failed");
    assert_eq!(outcome["done"], true);
    assert_eq!(outcome["error_truncated"], true);
    let saved = outcome["error"].as_str().unwrap();
    assert!(saved.len() <= 2048 && error.starts_with(saved));
    let rule = Retention::Before {
        utc_ms: chrono::Utc::now().timestamp_millis() + 1000,
    };
    assert_eq!(store.clean(&child, &rule, true).unwrap().pinned, 0);
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.agent_task(&global, &task.run_id, 1024).unwrap().1,
        outcome
    );
    let clean = store
        .clean(
            &child,
            &Retention::Before {
                utc_ms: chrono::Utc::now().timestamp_millis() + 1000,
            },
            false,
        )
        .unwrap();
    assert_eq!(clean.pinned, 0);
    assert!(clean.deleted > 0);
}

#[tokio::test]
async fn queried_and_waited_results_survive_cleaning_until_the_parent_finishes() {
    for waiting in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&temp.path().join("data/tasks.db")).unwrap());
        let global = store.agent("owner", "desktop", None).unwrap();
        let child = store.agent("owner", "desktop", Some("session")).unwrap();
        let root = store
            .accept_user(&global, "root", "coordinate", json!({}))
            .unwrap();
        let task = store
            .delegate(
                &child,
                &root.root_user_message_id,
                "task",
                "work",
                json!({}),
                None,
            )
            .unwrap();
        let reply_id = uuid::Uuid::now_v7().to_string();
        store
            .append_identified(
                &child,
                "assistant",
                &root.root_user_message_id,
                &reply_id,
                json!({"text":"long retained final result"}),
                Some(&task.run_id),
            )
            .unwrap();
        store.finish_run(&child, &task.run_id, "completed").unwrap();
        let host = AgentHost::new(store.clone(), tokio::runtime::Handle::current());
        let (_cancel, receiver) = tokio::sync::watch::channel(false);
        let context = ToolContext {
            history_unit_id: "unit".into(),
            vision: false,
            scope: global.clone(),
            run_id: root.run_id.clone(),
            root_user_message_id: root.root_user_message_id,
            action_id: "query-task".into(),
            max_read_bytes: 4,
            budget: Arc::new(Budget::new(30, 10, 10000, global.clone())),
            cancel: receiver,
            execution_gate: Arc::new(Mutex::new(true)),
        };
        let output = if waiting {
            host.wait_agent_task(&context, json!({"task_id":task.run_id,"timeout_ms":30000}))
                .await
                .unwrap()
        } else {
            host.get_agent_task(&context, json!({"task_id":task.run_id}))
                .unwrap()
        }
        .value;
        assert_eq!(output["result_truncated"], true);
        let rule = Retention::Before {
            utc_ms: chrono::Utc::now().timestamp_millis() + 1000,
        };
        let clean = store.clean(&child, &rule, false).unwrap();
        assert_eq!(
            clean.pinned, 2,
            "protect the answer and its completion report"
        );
        let body = store
            .record_page(
                &global,
                output["result_record_id"].as_str().unwrap(),
                "body",
                None,
                1024,
            )
            .unwrap();
        assert!(
            body["body"]
                .as_str()
                .unwrap()
                .contains("long retained final result")
        );
        assert_eq!(
            store.agent_task(&global, &task.run_id, 1024).unwrap().1["result_available"],
            true
        );
        store
            .finish_run(&global, &root.run_id, "completed")
            .unwrap();
        store.clean(&child, &rule, false).unwrap();
        assert_eq!(
            store.agent_task(&global, &task.run_id, 1024).unwrap().1["result_available"],
            false
        );
    }
}

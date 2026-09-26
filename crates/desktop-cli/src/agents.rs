use ai_terminal_agent::Client;
use ai_terminal_agent_runtime::store::Retention;
use ai_terminal_protocol::local::{Operation, Request};
use anyhow::{Context, Result, ensure};
use chrono::TimeZone;
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;
#[derive(Subcommand)]
pub enum AgentAction {
    List,
    Show {
        id: Option<String>,
        #[arg(long)]
        session: Option<String>,
    },
    Send {
        id: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        message: String,
        #[arg(long)]
        allow_input: bool,
        #[arg(long)]
        request_id: Option<String>,
    },
    Stop {
        id: Option<String>,
        #[arg(long)]
        session: Option<String>,
    },
    History {
        id: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        cursor: Option<String>,
    },
    Record {
        record_id: String,
        #[arg(long)]
        session: Option<String>,
        #[arg(long, default_value = "body")]
        part: String,
        #[arg(long)]
        cursor: Option<String>,
    },
}
#[derive(Subcommand)]
pub enum HistoryAction {
    Clean {
        #[command(flatten)]
        selector: Selector,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Retention {
        #[command(subcommand)]
        command: RetentionAction,
    },
}
#[derive(Subcommand)]
pub enum RetentionAction {
    Set {
        #[command(flatten)]
        selector: Selector,
        #[arg(long)]
        session: Option<String>,
    },
    Show {
        #[arg(long)]
        session: Option<String>,
    },
    Off {
        #[arg(long)]
        session: Option<String>,
    },
}
#[derive(Args)]
#[group(required = true, multiple = false)]
pub struct Selector {
    #[arg(long)]
    older_than: Option<String>,
    #[arg(long)]
    before: Option<String>,
    #[arg(long)]
    keep_last: Option<u64>,
}
impl Selector {
    fn resolve(self) -> Result<Retention> {
        if let Some(days) = self.older_than {
            let days = days
                .strip_suffix('d')
                .context("older-than requires Nd")?
                .parse::<u32>()?;
            ensure!(days > 0 && days <= 36500, "invalid_retention_days");
            return Ok(Retention::OlderThan { days });
        }
        if let Some(date) = self.before {
            let timestamp = if let Ok(date) = chrono::DateTime::parse_from_rfc3339(&date) {
                date.timestamp_millis()
            } else {
                let date = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")?
                    .and_hms_opt(0, 0, 0)
                    .unwrap();
                chrono::Local
                    .from_local_datetime(&date)
                    .single()
                    .context("ambiguous local date; use RFC3339 with timezone")?
                    .timestamp_millis()
            };
            return Ok(Retention::Before { utc_ms: timestamp });
        }
        let count = self.keep_last.context("retention_selector_required")?;
        ensure!(count > 0, "invalid_keep_last");
        Ok(Retention::KeepLast { count })
    }
}
fn call(
    state: Option<PathBuf>,
    session: Option<String>,
    mut value: Value,
    json_output: bool,
) -> Result<u32> {
    value["version"] = json!(1);
    let root = state
        .map(Ok)
        .unwrap_or_else(ai_terminal_agent::default_state_dir)?;
    let client = Client::ensure(&root, &std::env::current_exe()?)?;
    let reply = client.call(Request {
        operation: Operation::Agent as i32,
        session: session.unwrap_or_default(),
        text: value.to_string(),
        ..Default::default()
    })?;
    let value: Value =
        serde_json::from_str(reply.history.first().context("missing_agent_response")?)?;
    if json_output {
        println!("{}", json!({"ok":true,"result":value}));
    } else {
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(0)
}
pub fn run(command: AgentAction, state: Option<PathBuf>, json_output: bool) -> Result<u32> {
    let (session, mut value, id) = match command {
        AgentAction::List => (None, json!({"action":"list"}), None),
        AgentAction::Show { id, session } => (session, json!({"action":"state"}), id),
        AgentAction::Send {
            id,
            session,
            message,
            allow_input,
            request_id,
        } => (
            session,
            json!({"action":"send","request_id":request_id.unwrap_or_else(ai_terminal_agent_runtime::request_id),"message":message,"allow_input":allow_input}),
            id,
        ),
        AgentAction::Stop { id, session } => (session, json!({"action":"cancel"}), id),
        AgentAction::History {
            id,
            session,
            cursor,
        } => (session, json!({"action":"history","cursor":cursor}), id),
        AgentAction::Record {
            record_id,
            session,
            part,
            cursor,
        } => (
            session,
            json!({"action":"record","record_id":record_id,"part":part,"cursor":cursor}),
            None,
        ),
    };
    if let Some(id) = id {
        value["agent_id"] = json!(id);
    }
    call(state, session, value, json_output)
}
pub fn history(command: HistoryAction, state: Option<PathBuf>, json_output: bool) -> Result<u32> {
    let (session, value) = match command {
        HistoryAction::Clean {
            selector,
            session,
            dry_run,
        } => (
            session,
            json!({"action":"clean","rule":selector.resolve()?,"dry_run":dry_run}),
        ),
        HistoryAction::Retention { command } => match command {
            RetentionAction::Set { selector, session } => (
                session,
                json!({"action":"retention","rule":selector.resolve()?}),
            ),
            RetentionAction::Show { session } => {
                (session, json!({"action":"retention","show":true}))
            }
            RetentionAction::Off { session } => (session, json!({"action":"retention","off":true})),
        },
    };
    call(state, session, value, json_output)
}

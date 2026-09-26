use ai_terminal_protocol::local::{Operation, Request};
use ai_terminal_security::account::DesktopAccountCommand;
use anyhow::{Context, Result, ensure};
use clap::Subcommand;
use std::{
    io::{self, Write},
    path::PathBuf,
};
#[derive(Subcommand)]
pub enum Management {
    Mcp {
        #[command(subcommand)]
        command: crate::extensions::Action,
    },
    Skills {
        #[command(subcommand)]
        command: crate::extensions::Action,
    },
    Agents {
        #[command(subcommand)]
        command: crate::agents::AgentAction,
    },
    History {
        #[command(subcommand)]
        command: crate::agents::HistoryAction,
    },
    Sessions {
        #[command(subcommand)]
        command: Sessions,
    },
    Daemon {
        #[command(subcommand)]
        command: Daemon,
    },
    Config {
        #[command(subcommand)]
        command: crate::configuration::ConfigAction,
    },
    Providers {
        #[command(subcommand)]
        command: crate::configuration::ResourceAction,
    },
    Models {
        #[command(subcommand)]
        command: crate::configuration::ResourceAction,
    },
    Auth {
        #[command(subcommand)]
        command: Auth,
    },
    Devices {
        #[command(subcommand)]
        command: Devices,
    },
}
#[derive(Subcommand)]
pub enum Sessions {
    Capture {
        id: String,
        #[arg(long)]
        output: PathBuf,
    },
    List,
    Show {
        id: String,
    },
    Attach {
        id: String,
    },
    Close {
        id: String,
    },
    History {
        id: String,
    },
}
#[derive(Subcommand)]
pub enum Daemon {
    Stop,
}
#[derive(Subcommand)]
pub enum Auth {
    Login {
        #[arg(long)]
        server: String,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        ca_file: Option<PathBuf>,
        /// Read one password line from stdin for local automation; never put it in argv.
        #[arg(long)]
        password_stdin: bool,
    },
    Status,
    Logout,
}
#[derive(Subcommand)]
pub enum Devices {
    List,
    Revoke { device_id: String },
}
pub fn run(command: Management, state: Option<PathBuf>, json: bool) -> Result<u32> {
    let command = match command {
        Management::Mcp { command } => return crate::extensions::run("mcp", command, state, json),
        Management::Skills { command } => {
            return crate::extensions::run("skills", command, state, json);
        }
        Management::Agents { command } => return crate::agents::run(command, state, json),
        Management::History { command } => return crate::agents::history(command, state, json),
        Management::Sessions { .. } | Management::Daemon { .. } => {
            anyhow::bail!("terminal command dispatch error")
        }
        Management::Config { command } => {
            return crate::configuration::config(command, state, json);
        }
        Management::Providers { command } => {
            return crate::configuration::resource("providers", command, state, json);
        }
        Management::Models { command } => {
            return crate::configuration::resource("models", command, state, json);
        }
        Management::Auth {
            command:
                Auth::Login {
                    server,
                    username,
                    name,
                    ca_file,
                    password_stdin,
                },
        } => {
            let username = if let Some(v) = username {
                v
            } else {
                print!("Username: ");
                io::stdout().flush()?;
                let mut v = String::new();
                io::stdin().read_line(&mut v)?;
                v.trim().to_owned()
            };
            let password = if password_stdin {
                let mut line = String::new();
                io::stdin().read_line(&mut line)?;
                line.trim_end_matches(['\r', '\n']).to_owned()
            } else {
                rpassword::prompt_password("Password: ")?
            };
            ensure!(!password.is_empty(), "password is required");
            let ca = ca_file
                .or_else(|| std::env::var_os("AI_TERMINAL_CA_FILE").map(PathBuf::from))
                .map(std::fs::read_to_string)
                .transpose()
                .context("read configured CA")?;
            DesktopAccountCommand::Login {
                server,
                ca,
                username,
                password,
                device_name: name.unwrap_or_else(|| "Desktop".into()),
            }
        }
        Management::Auth {
            command: Auth::Status,
        } => DesktopAccountCommand::Status,
        Management::Auth {
            command: Auth::Logout,
        } => DesktopAccountCommand::Logout,
        Management::Devices {
            command: Devices::List,
        } => DesktopAccountCommand::Devices,
        Management::Devices {
            command: Devices::Revoke { device_id },
        } => DesktopAccountCommand::Revoke { device_id },
    };
    let dir = state
        .map(Ok)
        .unwrap_or_else(ai_terminal_agent::default_state_dir)?;
    let client = ai_terminal_agent::Client::ensure(&dir, &std::env::current_exe()?)?;
    let reply = client.call(Request {
        operation: Operation::Account as i32,
        text: serde_json::to_string(&command)?,
        ..Request::default()
    })?;
    if json {
        println!("{}", serde_json::json!({"ok":true,"result":reply.history}));
    } else {
        for line in reply.history {
            println!("{line}")
        }
    }
    Ok(0)
}

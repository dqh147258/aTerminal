use ai_terminal_protocol::local::{Operation, Request};
use ai_terminal_security::account::DesktopAccountCommand;
use anyhow::{Context, Result};
use clap::Subcommand;
use std::{
    io::{self, Write},
    path::PathBuf,
};
#[derive(Subcommand)]
pub enum Management {
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
    },
    Status,
    Logout,
}
#[derive(Subcommand)]
pub enum Devices {
    List,
    Revoke { device_id: String },
}
pub fn run(command: Management, state: Option<PathBuf>) -> Result<u32> {
    let command = match command {
        Management::Auth {
            command:
                Auth::Login {
                    server,
                    username,
                    name,
                    ca_file,
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
            let password = rpassword::prompt_password("Password: ")?;
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
    let dir = state.unwrap_or_else(ai_terminal_agent::default_state_dir);
    let client = ai_terminal_agent::Client::ensure(&dir, &std::env::current_exe()?)?;
    let reply = client.call(Request {
        operation: Operation::Account as i32,
        text: serde_json::to_string(&command)?,
        ..Request::default()
    })?;
    for line in reply.history {
        println!("{line}")
    }
    Ok(0)
}

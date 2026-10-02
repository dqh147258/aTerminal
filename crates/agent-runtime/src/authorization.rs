//! Pure, conservative application-level action policy, not an OS sandbox.
//!
//! The Host constructs descriptors from its authenticated scope, tool registry and
//! fresh terminal observations, never by deserializing model-provided proof fields.
//! Identity/session checks, cancellation, manual-input fences and execution budgets
//! remain mandatory even with full authorization. Reassess immediately before an
//! effect, and only match a persisted rule when `can_always` is true. A fingerprint
//! is an equality key, not a capability or proof of user approval.
//!
//! Shell classification recognizes a small positive language. It cannot constrain
//! aliases, shell hooks, changed binaries, symlinks or device files at the OS layer.
//! The Host must not claim otherwise. Arbitrary permission-store access from a
//! fully authorized shell cannot be prevented here; authorization RPCs must also
//! reject model-originated callers independently of this classifier.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    #[default]
    Ask,
    ReadOnly,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Once,
    Always,
    Deny,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Safe,
    RequiresApproval,
    /// Never overridden by full authorization, once approvals or persisted rules.
    Forbidden,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolSource {
    Builtin,
    Mcp,
    Skill,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellDialect {
    Bash,
    Zsh,
}

/// Trusted observations, not hints supplied by a model or inferred from a screen.
/// The Host must validate the same revision under its execution fence at dispatch.
#[derive(Clone, Debug)]
pub struct ShellProof {
    pub dialect: ShellDialect,
    pub at_prompt: bool,
    pub input_buffer_empty: bool,
    pub observed_revision: u64,
    pub current_revision: u64,
}

/// Intentionally has no Deserialize implementation. All metadata except arguments
/// comes from authenticated Host state; arguments must be the *complete* validated
/// call, including defaults actually used by the dispatcher.
#[derive(Clone, Debug)]
pub struct ActionDescriptor {
    pub account_id: String,
    pub desktop_id: String,
    pub tool: String,
    pub source: ToolSource,
    /// Stable built-in registry, MCP server or skill package identity, not a label.
    pub source_id: String,
    /// Effective version/content/configuration identity; None means unknown.
    pub tool_version: Option<String>,
    /// Exact session or other capability target. Do not use a mutable display name.
    pub target: String,
    /// Actual observed absolute cwd. This is a match scope, not a directory sandbox.
    pub cwd: Option<String>,
    pub arguments: Value,
    /// Trusted digest of effective executable/script/MCP content and configuration.
    /// A command name, user-provided version or MCP annotation is not sufficient.
    pub execution_identity: Option<String>,
    pub shell_proof: Option<ShellProof>,
    /// Set by Host routing for every authorization-management operation.
    pub permission_management: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Assessment {
    pub risk: Risk,
    pub reason: String,
    /// Digest of original data, never a digest of the display preview.
    pub fingerprint: String,
    /// Readable, bounded, selectively redacted display. Never execute this text.
    pub redacted_preview: String,
    pub can_always: bool,
}

pub fn assess(action: &ActionDescriptor) -> Assessment {
    let (risk, reason) = classify(action);
    Assessment {
        risk,
        reason: reason.into(),
        fingerprint: stable_fingerprint(action),
        redacted_preview: redacted_preview(&action.arguments),
        can_always: risk != Risk::Forbidden && permanent_scope_known(action),
    }
}

fn classify(action: &ActionDescriptor) -> (Risk, &'static str) {
    if action.permission_management
        || matches!(
            action.tool.as_str(),
            "set_permissions" | "resolve" | "revoke_rule"
        )
        || contains_permission_command(action)
    {
        return (Risk::Forbidden, "model_permission_management_forbidden");
    }
    if action.source != ToolSource::Builtin {
        return (Risk::RequiresApproval, "extension_or_unknown_tool");
    }
    // The registry source must be genuine. A model's choice of a familiar name is
    // insufficient. The dispatcher also validates each tool's schema and scope.
    if !nonempty(&action.source_id) || !action.arguments.is_object() {
        return (Risk::RequiresApproval, "unverified_tool_descriptor");
    }
    if matches!(
        action.tool.as_str(),
        "list_sessions"
            | "get_terminal_state"
            | "read_terminal"
            | "read_record"
            | "skills_search"
            | "skills_read"
            | "mcp_tools"
            | "get_agent_state"
            | "get_agent_task"
            | "wait_agent_task"
            | "wait"
            | "get_command_result"
            | "wait_command"
            | "list_agent_tasks"
            | "get_agent_tasks"
            | "wait_agent_tasks"
            | "ask_user"
            | "search_history"
            | "wait_terminal"
            | "get_capabilities"
    ) {
        // mcp_tools may start an already user-enabled server to list its catalog.
        // This is an existing configured lifecycle, not arbitrary MCP tool access.
        return (Risk::Safe, "builtin_observation_or_wait");
    }
    if action.tool != "run_command" {
        return (Risk::RequiresApproval, "effectful_or_raw_input");
    }
    let Some(proof) = &action.shell_proof else {
        return (Risk::RequiresApproval, "shell_proof_missing");
    };
    if !proof.at_prompt
        || !proof.input_buffer_empty
        || proof.observed_revision != proof.current_revision
    {
        return (Risk::RequiresApproval, "shell_input_state_unproven");
    }
    let Some(cwd) = action.cwd.as_deref().filter(|cwd| absolute_cwd(cwd)) else {
        return (Risk::RequiresApproval, "cwd_unknown");
    };
    let Some(command) = action.arguments.get("command").and_then(Value::as_str) else {
        return (Risk::RequiresApproval, "complete_command_missing");
    };
    // These are the only execution-affecting fields supported by this classifier.
    // Future flags (e.g. environment, interpreter, prelude) require policy review.
    if action.arguments.as_object().is_none_or(|args| {
        args.keys()
            .any(|key| !matches!(key.as_str(), "command" | "session_id"))
    }) {
        return (Risk::RequiresApproval, "unknown_command_parameter");
    }
    match simple_words(command) {
        Some(words) if safe_command(&words, cwd) => (Risk::Safe, "recognized_read_command"),
        _ => (Risk::RequiresApproval, "command_outside_positive_language"),
    }
}

fn nonempty(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_control)
}

fn absolute_cwd(cwd: &str) -> bool {
    // POSIX absolute or a fully qualified Windows drive/UNC directory. Do not
    // canonicalize: changing spelling must not broaden an existing exact rule.
    nonempty(cwd)
        && (cwd.starts_with('/')
            || (cwd.len() >= 3
                && cwd.as_bytes()[0].is_ascii_alphabetic()
                && cwd.as_bytes()[1] == b':'
                && matches!(cwd.as_bytes()[2], b'/' | b'\\'))
            || (cwd.starts_with("\\\\") && cwd[2..].contains('\\')))
}

fn known(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(nonempty)
}

fn permanent_scope_known(action: &ActionDescriptor) -> bool {
    if !nonempty(&action.account_id)
        || !nonempty(&action.desktop_id)
        || !nonempty(&action.tool)
        || !nonempty(&action.source_id)
        || !nonempty(&action.target)
        || !known(&action.tool_version)
        || !action.cwd.as_deref().is_some_and(absolute_cwd)
        || action.source == ToolSource::Unknown
        || !action.arguments.is_object()
        || matches!(action.tool.as_str(), "input_text" | "send_keys")
    {
        return false;
    }
    // A registry build version alone cannot pin an external program or extension.
    if action.source != ToolSource::Builtin
        || matches!(
            action.tool.as_str(),
            "run_command" | "mcp_call" | "skill_action"
        )
    {
        return known(&action.execution_identity);
    }
    true
}

/// Stable v1 equality digest. Canonicalization sorts object keys recursively while
/// preserving all array order, string bytes, nulls and numeric representations.
/// No trimming, path normalization, shell rewriting or redaction is performed.
/// Revisions are transient execution fences and deliberately not part of a rule.
/// Changing this policy's semantics requires a new namespace to invalidate rules.
pub fn stable_fingerprint(action: &ActionDescriptor) -> String {
    let material = json!({
        "policy": "aterminal.authorization.v1",
        "account_id": action.account_id,
        "desktop_id": action.desktop_id,
        "tool": action.tool,
        "source": match action.source {
            ToolSource::Builtin => "builtin", ToolSource::Mcp => "mcp",
            ToolSource::Skill => "skill", ToolSource::Unknown => "unknown",
        },
        "source_id": action.source_id,
        "tool_version": action.tool_version,
        "target": action.target,
        "cwd": action.cwd,
        "arguments": action.arguments,
        "execution_identity": action.execution_identity,
        "shell_dialect": action.shell_proof.as_ref().map(|proof| match proof.dialect {
            ShellDialect::Bash => "bash", ShellDialect::Zsh => "zsh",
        }),
        "permission_management": action.permission_management,
    });
    let mut canonical = String::new();
    canonical_json(&material, &mut canonical);
    format!("v1:{}", blake3::hash(canonical.as_bytes()).to_hex())
}

fn canonical_json(value: &Value, output: &mut String) {
    match value {
        Value::Object(fields) => {
            output.push('{');
            let mut keys: Vec<_> = fields.keys().collect();
            keys.sort_unstable();
            for (index, key) in keys.into_iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                output.push_str(&serde_json::to_string(key).expect("JSON string serialization"));
                output.push(':');
                canonical_json(&fields[key], output);
            }
            output.push('}');
        }
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                canonical_json(value, output);
            }
            output.push(']');
        }
        _ => output.push_str(&value.to_string()),
    }
}

/// Lexer for a deliberately small subset shared by Bash and Zsh. Only ASCII
/// spaces separate words; balanced single/double quotes group literal characters.
/// Expansions/metacharacters are rejected even inside quotes. Backslash escaping,
/// concatenated quoted fragments and non-ASCII are unsupported and ask the user.
/// This is not a shell parser and must never be used to claim arbitrary shell safety.
fn simple_words(command: &str) -> Option<Vec<String>> {
    if command.is_empty() || command.len() > 8192 || !command.is_ascii() {
        return None;
    }
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    let mut closed = false;
    for ch in command.chars() {
        if ch.is_control()
            || matches!(
                ch,
                '$' | '`'
                    | '\\'
                    | ';'
                    | '&'
                    | '|'
                    | '<'
                    | '>'
                    | '('
                    | ')'
                    | '{'
                    | '}'
                    | '['
                    | ']'
                    | '*'
                    | '?'
                    | '~'
                    | '!'
                    | '#'
            )
        {
            return None;
        }
        if let Some(delimiter) = quote {
            if ch == delimiter {
                quote = None;
                closed = true;
            } else if matches!(ch, '\'' | '"') {
                return None;
            } else {
                word.push(ch);
            }
        } else if ch == ' ' {
            if started {
                result.push(std::mem::take(&mut word));
                started = false;
                closed = false;
            }
        } else if closed {
            return None;
        } else if matches!(ch, '\'' | '"') {
            if started {
                return None;
            }
            quote = Some(ch);
            started = true;
        } else {
            started = true;
            word.push(ch);
        }
    }
    if quote.is_some() {
        return None;
    }
    if started {
        result.push(word);
    }
    (!result.is_empty() && result.len() <= 128).then_some(result)
}

fn safe_command(words: &[String], cwd: &str) -> bool {
    let program = words[0].as_str();
    let args = &words[1..];
    // No PATH wrappers, explicit binary paths, interpreters, git configuration,
    // pagers or implicit network utilities. Every supported option is enumerated.
    if program == "pwd" {
        return args.is_empty() || (args.len() == 1 && matches!(args[0].as_str(), "-L" | "-P"));
    }
    let allowed_flags: &[&str] = match program {
        "ls" => &[
            "-a", "-A", "-l", "-h", "-d", "-F", "-1", "-n", "-la", "-al", "-lh", "-lah", "-alh",
        ],
        "cat" => &["-n", "-b", "-s", "-v", "-E", "-T"],
        "head" | "tail" => &[],
        "wc" => &["-c", "-l", "-w", "-m"],
        _ => return false,
    };
    let mut operands = 0;
    let mut options = true;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if options && arg == "--" {
            options = false;
        } else if options && allowed_flags.contains(&arg) {
            // Exact positive option match.
        } else if options && matches!(program, "head" | "tail") && matches!(arg, "-n" | "-c") {
            index += 1;
            if args.get(index).is_none_or(|count| {
                count.is_empty()
                    || !count.bytes().all(|b| b.is_ascii_digit())
                    || count.parse::<u32>().map_or(true, |n| n > 1_000_000)
            }) {
                return false;
            }
        } else if arg.starts_with('-') || !ordinary_path(arg, cwd) {
            // Reject option-like operands even after -- to avoid utility variance.
            return false;
        } else {
            options = false;
            operands += 1;
        }
        index += 1;
    }
    if program == "ls" {
        operands > 0 || ordinary_path(".", cwd)
    } else {
        operands > 0 // No implicit stdin or '-' reads from an interactive terminal.
    }
}

fn ordinary_path(path: &str, cwd: &str) -> bool {
    if path.is_empty()
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._- +".contains(&b))
        || !cwd.starts_with('/')
        || path.starts_with("//")
    {
        return false;
    }
    // Lexically exclude common pseudo-filesystems, including relative/../ forms.
    // Symlink resolution and filesystem access are intentionally not done here.
    let joined = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{cwd}/{path}")
    };
    let mut parts = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    !matches!(parts.first(), Some(&"dev" | &"proc" | &"sys"))
}

fn contains_permission_command(action: &ActionDescriptor) -> bool {
    if !matches!(action.tool.as_str(), "run_command" | "input_text") {
        return false;
    }
    let field = if action.tool == "run_command" {
        "command"
    } else {
        "text"
    };
    let Some(command) = action.arguments.get(field).and_then(Value::as_str) else {
        return false;
    };
    // Defense in depth for obvious direct/compound CLI calls. This is deliberately
    // not advertised as a complete detection of arbitrary computed shell programs.
    let words: Vec<_> = command
        .split(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '_' | '-'))
        .filter(|word| !word.is_empty())
        .collect();
    words
        .iter()
        .any(|word| word.eq_ignore_ascii_case("aterminal"))
        && words.iter().any(|word| {
            matches!(
                *word,
                "set_permissions"
                    | "set-permissions"
                    | "permissions"
                    | "resolve"
                    | "revoke_rule"
                    | "revoke-rule"
            )
        })
}

/// Readable display of the complete call: ordinary commands, arguments and paths
/// remain visible. Sensitive fields, explicit credential syntax and URL userinfo
/// are redacted. Unknown free text is not assumed secret: the Host must provide
/// its known secret values using `redacted_preview_with_secrets` when available.
/// Never execute a preview or use it as the equality key for an approval.
pub fn redacted_preview(arguments: &Value) -> String {
    redacted_preview_with_secrets(arguments, &[])
}

/// Bounded JSON display, at most 4096 UTF-8 bytes. A truncated display is an
/// envelope with `truncated:true`, `requires_details:true` and a redacted prefix
/// in `preview`. It is not sufficient for an informed approval: the Host/UI must
/// present the full redacted details through an authenticated, bounded detail
/// read before approval. Truncation happens *after* redaction, never before it.
pub fn redacted_preview_with_secrets(arguments: &Value, known_secrets: &[String]) -> String {
    let text = redacted_details_with_secrets(arguments, known_secrets).to_string();
    if text.len() <= 4096 {
        return text;
    }
    // The prefix is a string, so the enclosing envelope remains valid JSON even
    // when truncation falls within a nested value. Account for JSON escaping too.
    let mut end = text.len().min(3900);
    loop {
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let preview = json!({
            "preview": &text[..end],
            "truncated": true,
            "requires_details": true,
        })
        .to_string();
        if preview.len() <= 4096 {
            return preview;
        }
        end = end.saturating_sub((preview.len() - 4096).max(1));
    }
}

/// Full redacted display for a Host's authenticated detail/pagination endpoint.
/// No preview truncation is performed here. The Host bounds input size and detail
/// responses, and retains the original call separately for execution/fingerprints.
/// Known secrets are trusted Host values, not a model-provided list. Literal and
/// percent-encoded occurrences are removed, including occurrences in object keys.
pub fn redacted_details_with_secrets(arguments: &Value, known_secrets: &[String]) -> Value {
    match arguments {
        Value::Object(fields) => {
            let mut output = serde_json::Map::new();
            for (key, value) in fields {
                let sensitive = sensitive_name(key);
                let mut display_key = redact_text(key, known_secrets);
                // Two secret-bearing keys may redact to the same label. Preserve
                // both displayed values rather than silently dropping a target.
                while output.contains_key(&display_key) {
                    display_key.push('#');
                }
                let display_value = if sensitive {
                    json!("[REDACTED]")
                } else {
                    redacted_details_with_secrets(value, known_secrets)
                };
                output.insert(display_key, display_value);
            }
            Value::Object(output)
        }
        Value::Array(values) => {
            let mut hide_next = false;
            Value::Array(
                values
                    .iter()
                    .map(|value| {
                        if hide_next {
                            hide_next = false;
                            return json!("[REDACTED]");
                        }
                        hide_next = value.as_str().is_some_and(|value| {
                            value.starts_with('-') && !value.contains('=') && sensitive_name(value)
                        });
                        redacted_details_with_secrets(value, known_secrets)
                    })
                    .collect(),
            )
        }
        Value::String(value) => json!(redact_text(value, known_secrets)),
        Value::Number(_) | Value::Bool(_)
            if known_secrets
                .iter()
                .any(|secret| !secret.is_empty() && secret == &arguments.to_string()) =>
        {
            json!("[REDACTED]")
        }
        _ => arguments.clone(),
    }
}

fn sensitive_name(name: &str) -> bool {
    let normalized: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|ch| ch.to_ascii_lowercase())
        .collect();
    normalized.contains("password")
        || normalized.contains("passwd")
        || normalized.contains("secret")
        || normalized.ends_with("token")
        || normalized.starts_with("token")
        || matches!(
            normalized.as_str(),
            "auth"
                | "authentication"
                | "authorization"
                | "proxyauthorization"
                | "cookie"
                | "cookies"
                | "setcookie"
                | "env"
                | "environment"
                | "environ"
                | "environmentvariables"
                | "apikey"
                | "xapikey"
                | "accesskey"
                | "accesskeyid"
                | "privatekey"
                | "signingkey"
                | "credentials"
                | "credential"
                | "oauth2bearer"
                | "passphrase"
        )
}

#[derive(Clone, Copy)]
struct DisplayToken {
    start: usize,
    end: usize,
}

/// Display-only spans. This scanner does not classify safety or parse executable
/// shell grammar. Keep quoting and compound syntax intact outside secret spans.
fn display_tokens(text: &str) -> Vec<DisplayToken> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            start.get_or_insert(index);
            escaped = true;
        } else if let Some(delimiter) = quote {
            if ch == delimiter {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"') {
            start.get_or_insert(index);
            quote = Some(ch);
        } else if ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '<' | '>') {
            if let Some(start) = start.take() {
                tokens.push(DisplayToken { start, end: index });
            }
        } else {
            start.get_or_insert(index);
        }
    }
    if let Some(start) = start {
        tokens.push(DisplayToken {
            start,
            end: text.len(),
        });
    }
    tokens
}

fn redact_text(text: &str, known_secrets: &[String]) -> String {
    let mut spans = Vec::<(usize, usize)>::new();
    for secret in known_secrets.iter().filter(|secret| !secret.is_empty()) {
        for (start, _) in text.match_indices(secret.as_str()) {
            spans.push((start, start + secret.len()));
        }
        let encoded: String = secret
            .bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                    (byte as char).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect();
        if encoded != *secret {
            let encoded_bytes = encoded.as_bytes();
            for (start, candidate) in text.as_bytes().windows(encoded_bytes.len()).enumerate() {
                let matches = encoded_bytes.iter().enumerate().all(|(index, expected)| {
                    let hex_position = (index > 0 && encoded_bytes[index - 1] == b'%')
                        || (index > 1 && encoded_bytes[index - 2] == b'%');
                    if hex_position {
                        expected.eq_ignore_ascii_case(&candidate[index])
                    } else {
                        *expected == candidate[index]
                    }
                });
                if matches {
                    spans.push((start, start + encoded_bytes.len()));
                }
            }
        }
    }
    let tokens = display_tokens(text);
    let network_cli = tokens.iter().any(|token| {
        matches!(
            text[token.start..token.end].trim_matches(['\'', '"']),
            "curl" | "wget" | "http" | "httpie"
        )
    });
    for (index, token) in tokens.iter().enumerate() {
        let raw = &text[token.start..token.end];
        let flag = raw
            .split('=')
            .next()
            .unwrap_or(raw)
            .trim_matches(['\'', '"']);
        if flag.starts_with('-')
            && (sensitive_name(flag)
                || matches!(flag, "--user" | "--proxy-user")
                || (network_cli && matches!(flag, "-u" | "-b")))
        {
            if let Some(offset) = raw.find('=') {
                spans.push((token.start + offset + 1, token.end));
            } else if let Some(next) = tokens.get(index + 1) {
                // Do not consume a token from the next compound command.
                if text[token.end..next.start].chars().all(char::is_whitespace) {
                    spans.push((next.start, next.end));
                }
            }
        } else if network_cli
            && (raw.starts_with("-u") || raw.starts_with("-b"))
            && raw.len() > 2
            && !raw.starts_with("--")
        {
            spans.push((token.start + 2, token.end));
        }
        if matches!(flag.to_ascii_lowercase().as_str(), "bearer" | "basic")
            && let Some(next) = tokens.get(index + 1)
            && text[token.end..next.start].chars().all(char::is_whitespace)
        {
            spans.push((next.start, next.end));
        }
    }
    // Explicit name=value and name:value syntax covers sensitive environment
    // assignments, JSON payload strings, HTTP headers and URL query parameters.
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_alphabetic() && bytes[index] != b'_' {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len()
            && (bytes[index].is_ascii_alphanumeric() || b"_-.".contains(&bytes[index]))
        {
            index += 1;
        }
        if !sensitive_name(&text[start..index]) {
            continue;
        }
        let mut delimiter = index;
        if bytes
            .get(delimiter)
            .is_some_and(|byte| matches!(byte, b'\'' | b'"'))
        {
            delimiter += 1;
        }
        while bytes.get(delimiter).is_some_and(u8::is_ascii_whitespace) {
            delimiter += 1;
        }
        if !bytes
            .get(delimiter)
            .is_some_and(|byte| matches!(byte, b'=' | b':'))
        {
            continue;
        }
        let mut value_start = delimiter + 1;
        while bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
            value_start += 1;
        }
        let Some(&first) = bytes.get(value_start) else {
            continue;
        };
        let mut value_end = value_start;
        if matches!(first, b'\'' | b'"') {
            value_start += 1;
            value_end = quoted_end(bytes, value_start, first);
        } else {
            while value_end < bytes.len()
                && !bytes[value_end].is_ascii_whitespace()
                && !b"&;|,)}]\"'<>".contains(&bytes[value_end])
            {
                value_end += 1;
            }
            if matches!(
                text[value_start..value_end].to_ascii_lowercase().as_str(),
                "bearer" | "basic"
            ) {
                value_start = value_end;
                while bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
                    value_start += 1;
                }
                value_end = value_start;
                while value_end < bytes.len()
                    && !bytes[value_end].is_ascii_whitespace()
                    && !b"&;|,)}]\"'<>".contains(&bytes[value_end])
                {
                    value_end += 1;
                }
            } else if text[start..index].eq_ignore_ascii_case("cookie") {
                // A quoted Cookie header may contain several semicolon-separated
                // cookies. Hide the whole header value inside its outer quote.
                if let Some(&quote) = start.checked_sub(1).and_then(|i| bytes.get(i))
                    && matches!(quote, b'\'' | b'"')
                {
                    value_end = quoted_end(bytes, value_start, quote);
                }
            }
        }
        if value_end > value_start {
            spans.push((value_start, value_end));
        }
    }
    // URL userinfo is sensitive regardless of the chosen username/parameter keys.
    for (scheme, _) in text.match_indices("://") {
        let start = scheme + 3;
        let end = text[start..]
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '/' | '?' | '#' | '\'' | '"'))
            .map_or(text.len(), |offset| start + offset);
        if let Some(offset) = text[start..end].rfind('@') {
            spans.push((start, start + offset));
        }
        let url_end = text[start..]
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '\'' | '"'))
            .map_or(text.len(), |offset| start + offset);
        if let Some(query) = text[end..url_end].find('?') {
            let query_start = end + query + 1;
            let mut offset = query_start;
            for parameter in text[query_start..url_end].split(['&', '#']) {
                if let Some((name, value)) = parameter.split_once('=')
                    && sensitive_name(&percent_decode_name(name))
                {
                    let start = offset + name.len() + 1;
                    spans.push((start, start + value.len()));
                }
                offset += parameter.len() + 1;
            }
        }
    }
    spans.sort_unstable();
    let mut merged = Vec::<(usize, usize)>::new();
    for (start, end) in spans {
        if start >= end {
            continue;
        }
        if let Some(previous) = merged.last_mut()
            && start <= previous.1
        {
            previous.1 = previous.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    let mut output = String::new();
    let mut cursor = 0;
    for (start, end) in merged {
        output.push_str(&text[cursor..start]);
        output.push_str("[REDACTED]");
        cursor = end;
    }
    output.push_str(&text[cursor..]);
    output
}

fn quoted_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut end = start;
    while end < bytes.len() {
        if bytes[end] == quote {
            break;
        }
        if bytes[end] == b'\\' {
            end += 1;
        }
        end = (end + 1).min(bytes.len());
    }
    end
}

fn percent_decode_name(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2]))
        {
            decoded.push(high * 16 + low);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(command: &str) -> ActionDescriptor {
        ActionDescriptor {
            account_id: "account-a".into(),
            desktop_id: "desktop-a".into(),
            tool: "run_command".into(),
            source: ToolSource::Builtin,
            source_id: "aterminal-builtins".into(),
            tool_version: Some("build-v1".into()),
            target: "session-a".into(),
            cwd: Some("/work/project".into()),
            arguments: json!({"command": command}),
            execution_identity: Some("trusted-executable-and-configuration-digest".into()),
            shell_proof: Some(ShellProof {
                dialect: ShellDialect::Bash,
                at_prompt: true,
                input_buffer_empty: true,
                observed_revision: 42,
                current_revision: 42,
            }),
            permission_management: false,
        }
    }

    #[test]
    fn recognized_commands_have_positive_argument_grammars() {
        for command in [
            "pwd",
            "pwd -P",
            "ls",
            "ls -la /work",
            "ls -lah ../elsewhere",
            "cat -- README.md",
            "cat 'a file.txt'",
            "cat \"a file.txt\"",
            "head -n 25 ./file",
            "tail -c 1024 /var/log/service.log",
            "wc -l file1 file2",
            "ls ./-file",
            "  pwd  ",
        ] {
            assert_eq!(assess(&action(command)).risk, Risk::Safe, "{command}");
        }
    }

    #[test]
    fn shell_expansions_and_controls_always_require_approval() {
        for command in [
            "ls; pwd",
            "ls && pwd",
            "ls || pwd",
            "ls | cat",
            "ls &",
            "ls\npwd",
            "ls\r",
            "ls\t",
            "ls\0",
            "ls\u{1b}[0m",
            "ls $(pwd)",
            "ls `pwd`",
            "ls $HOME",
            "ls ${HOME}",
            "ls $((1+1))",
            "ls <(pwd)",
            "cat > file",
            "cat < file",
            "ls 2>err",
            "ls >>file",
            "cat <<EOF",
            "ls *",
            "ls ?",
            "ls [ab]",
            "ls {a,b}",
            "ls ~",
            "ls !1",
            "ls #comment",
            "(pwd)",
            "{ pwd; }",
            "ls \\x",
            "ls \\\npwd",
            "ls '=pwd'",
            "ls \"$HOME\"",
            "ls '$(pwd)'",
            "ls 'a;b'",
            "ls 'a\\b'",
            "l's'",
            "'ls'x",
            "ls \"unterminated",
            "ls 'unterminated",
            "ls \"\"suffix",
            "ls $'file'",
            "ls —help",
            "ls ＄HOME",
            "ls \u{a0}file",
            "ls \u{202e}file",
            "ls 中文",
            "ls\u{2028}pwd",
            "ls =pwd",
        ] {
            assert_eq!(
                assess(&action(command)).risk,
                Risk::RequiresApproval,
                "{command:?}"
            );
        }
    }

    #[test]
    fn programs_options_and_path_tricks_fail_closed() {
        for command in [
            "",
            " ",
            "unknown",
            "/bin/ls",
            "./ls",
            "../bin/ls",
            "LS",
            "command ls",
            "env ls",
            "PATH=/tmp ls",
            "sudo ls",
            "sh -c pwd",
            "python -c print(1)",
            "git status",
            "git -c core.pager=sh log",
            "rm file",
            "cp a b",
            "curl host",
            "ls --help",
            "ls --color=always",
            "ls -I file",
            "cat",
            "cat -",
            "cat -- -",
            "cat --help",
            "head -n -1 file",
            "head -n 1000001 file",
            "head -n 1x file",
            "head -n file",
            "head -n 1",
            "tail -f file",
            "tail --pid=1 file",
            "wc --files0-from=file",
            "ls file -l",
            "cat ''",
            "cat -n -- -s",
            "cat /dev/tty",
            "cat /proc/self/fd/0",
            "cat /sys/kernel/foo",
            "cat ../../dev/tty",
            "cat //host/file",
            "ls ../..//dev",
            "ls /proc",
            "cat host:file",
            "cat https://host/file",
            "ls /dev/../proc/self",
        ] {
            assert_eq!(
                assess(&action(command)).risk,
                Risk::RequiresApproval,
                "{command:?}"
            );
        }
        let mut a = action("ls");
        a.cwd = Some("/dev".into());
        assert_eq!(assess(&a).risk, Risk::RequiresApproval);
    }

    #[test]
    fn shell_proof_must_be_fresh_complete_and_not_model_arguments() {
        let mut a = action("pwd");
        a.shell_proof = None;
        a.arguments["safe"] = json!(true);
        a.arguments["at_prompt"] = json!(true);
        assert_eq!(assess(&a).risk, Risk::RequiresApproval);
        for variant in 0..5 {
            let mut a = action("pwd");
            match variant {
                0 => a.shell_proof.as_mut().unwrap().at_prompt = false,
                1 => a.shell_proof.as_mut().unwrap().input_buffer_empty = false,
                2 => a.shell_proof.as_mut().unwrap().current_revision += 1,
                3 => a.arguments["environment"] = json!({"PATH":"/tmp"}),
                _ => a.arguments["interpreter"] = json!("custom-shell"),
            }
            assert_eq!(assess(&a).risk, Risk::RequiresApproval);
        }
        for cwd in [
            None,
            Some(""),
            Some("relative"),
            Some("C:relative"),
            Some("/work\n"),
        ] {
            let mut a = action("pwd");
            a.cwd = cwd.map(str::to_owned);
            let assessment = assess(&a);
            assert_eq!(assessment.risk, Risk::RequiresApproval);
            assert!(!assessment.can_always);
        }
    }

    #[test]
    fn raw_input_enter_and_tui_never_auto_allow_or_persist() {
        for (tool, arguments) in [
            ("input_text", json!({"text":"pwd","submit":true})),
            ("input_text", json!({"text":"pwd\n","submit":false})),
            ("input_text", json!({"text":"p"})),
            ("input_text", json!({"text":"wd"})),
            ("send_keys", json!({"key":"enter"})),
            ("send_keys", json!({"key":"c","modifiers":["ctrl"]})),
        ] {
            let mut a = action("pwd");
            a.tool = tool.into();
            a.arguments = arguments;
            let assessment = assess(&a);
            assert_eq!(assessment.risk, Risk::RequiresApproval);
            assert!(!assessment.can_always);
        }
    }

    #[test]
    fn builtin_reads_are_safe_but_familiar_extension_names_are_not() {
        for tool in [
            "read_terminal",
            "read_record",
            "wait",
            "get_command_result",
            "wait_command",
            "list_agent_tasks",
            "get_agent_tasks",
            "wait_agent_tasks",
            "ask_user",
            "search_history",
            "wait_terminal",
            "get_capabilities",
            "mcp_tools",
        ] {
            let mut a = action("pwd");
            a.tool = tool.into();
            a.arguments = json!({"mode":"delta"});
            a.shell_proof = None;
            assert_eq!(assess(&a).risk, Risk::Safe, "{tool}");
            for source in [ToolSource::Mcp, ToolSource::Skill, ToolSource::Unknown] {
                a.source = source;
                a.arguments["readOnlyHint"] = json!(true);
                assert_eq!(assess(&a).risk, Risk::RequiresApproval, "{tool}");
            }
        }
        for tool in [
            "skill_action",
            "mcp_call",
            "cancel_agent_task",
            "send_agent_message",
            "new_tool",
        ] {
            let mut a = action("pwd");
            a.tool = tool.into();
            assert_eq!(assess(&a).risk, Risk::RequiresApproval);
        }
    }

    #[test]
    fn permission_management_is_forbidden_even_with_exact_identity() {
        let mut a = action("pwd");
        a.permission_management = true;
        assert_eq!(assess(&a).risk, Risk::Forbidden);
        assert!(!assess(&a).can_always);
        for tool in ["set_permissions", "resolve", "revoke_rule"] {
            let mut a = action("pwd");
            a.tool = tool.into();
            assert_eq!(assess(&a).risk, Risk::Forbidden);
            assert!(!assess(&a).can_always);
        }
        for command in [
            "aTerminal agents permissions --full true",
            "/opt/bin/aTerminal agents resolve id always",
            "env aTerminal agents revoke-rule id",
            "pwd; aTerminal agents set-permissions --full true",
        ] {
            let a = action(command);
            assert_eq!(assess(&a).risk, Risk::Forbidden);
            assert!(!assess(&a).can_always);
        }
    }

    #[test]
    fn canonical_json_order_is_stable_but_all_action_parameters_matter() {
        let mut first = action("curl host");
        first.arguments = serde_json::from_str(r#"{"z":2,"a":{"y":[1,2],"b":"secret"}}"#).unwrap();
        let mut reordered = first.clone();
        reordered.arguments =
            serde_json::from_str(r#"{"a":{"b":"secret","y":[1,2]},"z":2}"#).unwrap();
        assert_eq!(stable_fingerprint(&first), stable_fingerprint(&reordered));
        reordered.arguments["a"]["y"] = json!([2, 1]);
        assert_ne!(stable_fingerprint(&first), stable_fingerprint(&reordered));
        let base = action("pwd");
        for variant in 0..14 {
            let mut changed = base.clone();
            match variant {
                0 => changed.account_id.push('2'),
                1 => changed.desktop_id.push('2'),
                2 => changed.tool.push('2'),
                3 => changed.source = ToolSource::Mcp,
                4 => changed.source_id.push('2'),
                5 => changed.tool_version = Some("build-v2".into()),
                6 => changed.target.push('2'),
                7 => changed.cwd = Some("/work/elsewhere".into()),
                8 => changed.arguments["command"] = json!("pwd -P"),
                9 => changed.execution_identity = Some("changed-binary".into()),
                10 => changed.shell_proof.as_mut().unwrap().dialect = ShellDialect::Zsh,
                11 => changed.permission_management = true,
                12 => changed.arguments["new_param"] = Value::Null,
                _ => changed.cwd = Some("/work/project/.".into()),
            }
            assert_ne!(
                stable_fingerprint(&base),
                stable_fingerprint(&changed),
                "variant {variant}"
            );
        }
        let mut later = base.clone();
        later.shell_proof.as_mut().unwrap().observed_revision += 1;
        later.shell_proof.as_mut().unwrap().current_revision += 1;
        assert_eq!(stable_fingerprint(&base), stable_fingerprint(&later));
    }

    #[test]
    fn unknown_scope_or_execution_version_disables_permanent_rules() {
        assert!(assess(&action("rm file")).can_always);
        for variant in 0..11 {
            let mut a = action("rm file");
            match variant {
                0 => a.cwd = None,
                1 => a.cwd = Some("".into()),
                2 => a.tool_version = None,
                3 => a.tool_version = Some(" ".into()),
                4 => a.account_id.clear(),
                5 => a.desktop_id.clear(),
                6 => a.target.clear(),
                7 => a.source_id.clear(),
                8 => a.execution_identity = None,
                9 => a.execution_identity = Some("".into()),
                _ => a.source = ToolSource::Unknown,
            }
            assert!(!assess(&a).can_always, "variant {variant}");
        }
        for tool in ["skill_action", "mcp_call"] {
            let mut a = action("pwd");
            a.tool = tool.into();
            a.execution_identity = None;
            assert!(!assess(&a).can_always);
        }
    }

    #[test]
    fn preview_redaction_never_collapses_original_fingerprints() {
        let first = action("curl https://user:secret@host/?token=secret&file=/backups");
        let mut second = first.clone();
        second.arguments["command"] =
            json!("curl https://user:different@host/?token=different&file=/backups");
        assert_eq!(
            assess(&first).redacted_preview,
            assess(&second).redacted_preview
        );
        assert_ne!(assess(&first).fingerprint, assess(&second).fingerprint);
        let preview: Value = serde_json::from_str(&assess(&first).redacted_preview).unwrap();
        let command = preview["command"].as_str().unwrap();
        assert!(command.contains("curl https://[REDACTED]@host/"));
        assert!(command.contains("file=/backups"));
        let mut different_target = first.clone();
        different_target.arguments["command"] =
            json!("curl https://user:secret@other-host/?token=secret&file=/production");
        assert_ne!(
            assess(&first).redacted_preview,
            assess(&different_target).redacted_preview
        );
        assert_ne!(
            assess(&first).fingerprint,
            assess(&different_target).fingerprint
        );
    }

    #[test]
    fn preview_preserves_dangerous_targets_and_ordinary_extension_arguments() {
        let arguments = json!({
            "command":"rm -rf '/production/a folder'; mv /source /destination > /logs/output",
            "cwd":"/work/project", "path":"/backups/2026", "text":"delete the old reports",
            "arguments":{"custom_target":"/production/database","dry_run":false,"count":123},
            "argv":["ssh","-p","22","user@host"],
        });
        let preview: Value = serde_json::from_str(&redacted_preview(&arguments)).unwrap();
        assert_eq!(preview, arguments);
        let a = action(arguments["command"].as_str().unwrap());
        assert_eq!(assess(&a).risk, Risk::RequiresApproval);
    }

    #[test]
    fn sensitive_structures_are_hidden_but_adjacent_targets_remain_visible() {
        let arguments = json!({
            "password":"hidden-password", "accessToken":"hidden-token", "api_key":12345,
            "arguments":{"custom_password":"hidden-custom","target":"/production"},
            "env":{"PATH":"hidden-env","NORMAL":"hidden-env-value"},
            "headers":{"Authorization":"hidden-auth","Cookie":"hidden-cookie","Accept":"application/json"},
            "auth":{"user":"hidden-user","password":"hidden-auth-password"},
            "destination":"/backup", "attempts":2,
        });
        let preview = redacted_preview(&arguments);
        assert!(!preview.contains("hidden-"));
        assert!(!preview.contains("12345"));
        let parsed: Value = serde_json::from_str(&preview).unwrap();
        assert_eq!(parsed["arguments"]["target"], "/production");
        assert_eq!(parsed["headers"]["Accept"], "application/json");
        assert_eq!(parsed["destination"], "/backup");
        assert_eq!(parsed["attempts"], 2);
    }

    #[test]
    fn command_credentials_redact_without_erasing_targets_or_shell_syntax() {
        for command in [
            "curl --password hidden-pass https://host/upload -o /backup/result",
            "curl --token=hidden-token https://host/upload -o /backup/result",
            "TOKEN='hidden token' curl https://host/upload -o /backup/result",
            "curl -H 'Authorization: Bearer hidden-auth' https://host/upload -o /backup/result",
            "curl -H 'Cookie: first=hidden-one; second=hidden-two' https://host/upload -o /backup/result",
            "curl -u user:hidden-auth https://host/upload -o /backup/result",
            "curl -uuser:hidden-auth https://host/upload -o /backup/result",
            "curl https://user:hidden-pass@host/upload?%74oken=hidden-query&path=/backup/result",
            r#"curl --data '{"password":"hidden-json","target":"/backup/result"}' https://host/upload"#,
        ] {
            let result = redacted_details_with_secrets(&json!({"command":command}), &[]);
            let display = result["command"].as_str().unwrap();
            assert!(!display.contains("hidden"), "{command} -> {display}");
            assert!(display.contains("host/upload"), "{display}");
            assert!(display.contains("/backup/result"), "{display}");
        }
        let args = json!({"argv":["deploy","--token","hidden-argv","/production"]});
        let preview = redacted_details_with_secrets(&args, &[]);
        assert_eq!(preview["argv"][2], "[REDACTED]");
        assert_eq!(preview["argv"][3], "/production");
    }

    #[test]
    fn host_known_secrets_redact_free_text_paths_and_encoded_occurrences() {
        let secrets = vec!["private-value".to_owned(), "PRIVATE:值".to_owned()];
        let args = json!({
            "command":"deploy /production --note private-value --target /backups",
            "path":"/private-value/export", "cwd":"/work/private-value",
            "ordinary":"This contains PRIVATE:值 in text",
            "url":"https://host/?value=PRIVATE%3a%E5%80%BC",
            "private-value":"/target/one", "private-value-private-value":"/target/two",
        });
        let original = args.clone();
        let preview = redacted_preview_with_secrets(&args, &secrets);
        assert!(!preview.contains("private-value"));
        assert!(!preview.contains("PRIVATE:值"));
        assert!(!preview.contains("PRIVATE%3a%E5%80%BC"));
        assert!(preview.contains("/production"));
        assert!(preview.contains("/backups"));
        assert!(preview.contains("/target/one"));
        assert!(preview.contains("/target/two"));
        assert_eq!(args, original);
        let with_empty_secret = redacted_preview_with_secrets(&args, &[String::new()]);
        assert_eq!(with_empty_secret, redacted_preview(&args));
        // Percent-looking Unicode query keys must not panic or hide ordinary values.
        let malformed = json!({"url":"https://host/?%中=ordinary&file=/backups"});
        assert_eq!(redacted_details_with_secrets(&malformed, &[]), malformed);
    }

    #[test]
    fn preview_truncation_is_explicit_and_full_redacted_details_retain_the_target() {
        let args = json!({
            "command":format!("deploy --note '{}' --token '{}' --target /production/database", "中文\\\"".repeat(2000), "hidden-long-secret".repeat(500)),
            "cwd":"/work/project",
        });
        let preview = redacted_preview(&args);
        assert!(preview.len() <= 4096);
        let parsed: Value = serde_json::from_str(&preview).unwrap();
        assert_eq!(parsed["truncated"], true);
        assert_eq!(parsed["requires_details"], true);
        assert!(parsed["preview"].as_str().unwrap().contains("deploy"));
        assert!(!preview.contains("hidden-long-secret"));
        let details = redacted_details_with_secrets(&args, &[]);
        let command = details["command"].as_str().unwrap();
        assert!(command.ends_with("--target /production/database"));
        assert!(command.contains("中文"));
        assert!(!command.contains("hidden-long-secret"));
    }

    #[test]
    fn preview_is_valid_json_and_assessment_serializes() {
        let a = action("rm -rf /production/database");
        let result = assess(&a);
        assert!(result.redacted_preview.len() <= 4096);
        let preview: Value = serde_json::from_str(&result.redacted_preview).unwrap();
        assert_eq!(preview["command"], "rm -rf /production/database");
        let serialized = serde_json::to_value(result).unwrap();
        assert_eq!(serialized["risk"], "requires_approval");
        assert!(
            serialized["fingerprint"]
                .as_str()
                .unwrap()
                .starts_with("v1:")
        );
        assert_eq!(
            serde_json::to_value(PermissionMode::ReadOnly).unwrap(),
            "read_only"
        );
        assert_eq!(serde_json::to_value(Decision::Always).unwrap(), "always");
        assert!(serde_json::from_value::<PermissionMode>(json!("full")).is_err());
    }
}

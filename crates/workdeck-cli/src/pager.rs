//! Safe plain-text pager fallback for `workdeck pager`.

use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;

use thiserror::Error;
use workdeck_diff::{SanitizeOptions, sanitize_terminal_text};

pub const TEXT_PAGER_ENV: &str = "WORKDECK_TEXT_PAGER";
const DEFAULT_TEXT_PAGER_COMMAND: &str = "less -R";

/// Detect whether generic pager stdin looks like a diff/patch that Workdeck should review.
#[must_use]
pub fn looks_like_patch_input(text: &str) -> bool {
    let normalized =
        sanitize_terminal_text(&text.replace("\r\n", "\n"), SanitizeOptions::default());
    let mut old_header = false;
    let mut new_header = false;
    for line in normalized.lines() {
        if line.starts_with("diff --git ") || line.starts_with("@@ ") {
            return true;
        }
        old_header |= line.starts_with("--- ");
        new_header |= line.starts_with("+++ ");
    }
    old_header && new_header
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPagerCommand {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub display_command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagerInvocation {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

pub trait PagerRunner: Send + Sync {
    fn run(&self, invocation: &PagerInvocation, text: &str) -> Result<i32, String>;
}

impl<F> PagerRunner for F
where
    F: Fn(&PagerInvocation, &str) -> Result<i32, String> + Send + Sync,
{
    fn run(&self, invocation: &PagerInvocation, text: &str) -> Result<i32, String> {
        self(invocation, text)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("Pager command failed: {command}")]
pub struct PagerError {
    pub command: String,
    pub detail: Option<String>,
}

impl PagerError {
    fn new(command: impl Into<String>, detail: Option<String>) -> Self {
        Self {
            command: command.into(),
            detail,
        }
    }
}

#[derive(Clone)]
pub struct PlainTextPagerContext {
    pub env: BTreeMap<String, String>,
    pub stdout_is_terminal: bool,
    pub runner: Arc<dyn PagerRunner>,
}

impl PlainTextPagerContext {
    #[must_use]
    pub fn current() -> Self {
        let env = std::env::vars().collect::<BTreeMap<_, _>>();
        Self {
            stdout_is_terminal: std::io::stdout().is_terminal()
                && env.get("TERM").is_none_or(|term| term != "dumb"),
            env,
            runner: Arc::new(NativePagerRunner),
        }
    }
}

struct NativePagerRunner;

impl PagerRunner for NativePagerRunner {
    fn run(&self, invocation: &PagerInvocation, text: &str) -> Result<i32, String> {
        let mut child = Command::new(&invocation.command)
            .args(&invocation.args)
            .envs(&invocation.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| error.to_string())?;
        if let Some(mut stdin) = child.stdin.take()
            && let Err(error) = stdin.write_all(text.as_bytes())
            && error.kind() != io::ErrorKind::BrokenPipe
        {
            return Err(error.to_string());
        }
        let status = child.wait().map_err(|error| error.to_string())?;
        Ok(status.code().unwrap_or(1))
    }
}

fn is_operator(character: char) -> bool {
    matches!(character, '>' | '<' | ';' | '|' | '&' | '(' | ')')
}

/// Split literal shell words without executing operators or expanding variables.
fn split_pager_command(command: &str) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = Quote::None;
    let mut characters = command.chars().peekable();
    while let Some(character) = characters.next() {
        match quote {
            Quote::Single => {
                if character == '\'' {
                    quote = Quote::None;
                } else {
                    word.push(character);
                }
            }
            Quote::Double => {
                if character == '"' {
                    quote = Quote::None;
                } else if character == '\\' && characters.peek() == Some(&'"') {
                    word.push(characters.next().expect("peeked quote"));
                } else {
                    word.push(character);
                }
            }
            Quote::None => match character {
                '\'' => quote = Quote::Single,
                '"' => quote = Quote::Double,
                '\\' => {
                    if let Some(next) = characters.next() {
                        word.push(next);
                    } else {
                        word.push('\\');
                    }
                }
                value if value.is_whitespace() => {
                    if !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                }
                value if is_operator(value) => {
                    if !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                    let mut operator = value.to_string();
                    if characters.peek() == Some(&value) && matches!(value, '>' | '<' | '|' | '&') {
                        operator.push(characters.next().expect("peeked operator"));
                    }
                    words.push(operator);
                }
                value => word.push(value),
            },
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

fn executable_name(command: &str) -> String {
    let name = command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    name.strip_suffix(".cmd")
        .or_else(|| name.strip_suffix(".exe"))
        .unwrap_or(&name)
        .to_owned()
}

fn parse_env_assignment(word: &str) -> Option<(&str, &str)> {
    let (name, value) = word.split_once('=')?;
    let mut characters = name.chars();
    let first = characters.next()?;
    (first == '_' || first.is_ascii_alphabetic())
        .then_some(())
        .filter(|_| {
            characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
        })
        .map(|_| (name, value))
}

fn resolve_pager_spec(command: &str) -> Option<ResolvedPagerCommand> {
    let words = split_pager_command(command);
    let mut env = BTreeMap::new();
    let mut command_index = 0;
    while let Some((name, value)) = words
        .get(command_index)
        .and_then(|word| parse_env_assignment(word))
    {
        env.insert(name.into(), value.into());
        command_index += 1;
    }
    if words
        .get(command_index)
        .is_some_and(|word| executable_name(word) == "env")
    {
        let mut env_index = command_index + 1;
        let mut wrapped = BTreeMap::new();
        while let Some((name, value)) = words
            .get(env_index)
            .and_then(|word| parse_env_assignment(word))
        {
            wrapped.insert(name.to_owned(), value.to_owned());
            env_index += 1;
        }
        if env_index < words.len() {
            command_index = env_index;
            env.extend(wrapped);
        }
    }
    let executable = words.get(command_index)?.clone();
    Some(ResolvedPagerCommand {
        command: executable,
        args: words[command_index + 1..].to_vec(),
        env,
        display_command: command.into(),
    })
}

/// Choose a plain-text pager while avoiding recursive `workdeck pager` launches.
#[must_use]
pub fn resolve_text_pager_spec(env: &BTreeMap<String, String>) -> ResolvedPagerCommand {
    let candidate = env.get(TEXT_PAGER_ENV).or_else(|| env.get("PAGER"));
    let resolved = candidate.and_then(|candidate| resolve_pager_spec(candidate));
    if let Some(resolved) = resolved.filter(|spec| executable_name(&spec.command) != "workdeck") {
        return resolved;
    }
    resolve_pager_spec(DEFAULT_TEXT_PAGER_COMMAND).expect("default pager command is valid")
}

#[must_use]
pub fn resolve_text_pager_command(env: &BTreeMap<String, String>) -> String {
    resolve_text_pager_spec(env).display_command
}

/// Stream plain text through a normal pager, or write directly for redirected stdout.
pub fn page_plain_text_with(
    text: &str,
    context: &PlainTextPagerContext,
    direct_output: &mut dyn Write,
) -> Result<(), PagerError> {
    if !context.stdout_is_terminal {
        let safe = sanitize_terminal_text(text, SanitizeOptions::default());
        direct_output
            .write_all(safe.as_bytes())
            .map_err(|error| PagerError::new("stdout", Some(error.to_string())))?;
        return Ok(());
    }

    let safe = sanitize_terminal_text(
        text,
        SanitizeOptions {
            preserve_ansi_style: true,
            ..SanitizeOptions::default()
        },
    );
    let spec = resolve_text_pager_spec(&context.env);
    let mut child_env = context.env.clone();
    child_env.extend(spec.env.clone());
    let invocation = PagerInvocation {
        command: spec.command,
        args: spec.args,
        env: child_env,
    };
    let code = context
        .runner
        .run(&invocation, &safe)
        .map_err(|detail| PagerError::new(&spec.display_command, Some(detail)))?;
    if code != 0 {
        return Err(PagerError::new(
            &spec.display_command,
            Some(format!("exit code {code}")),
        ));
    }
    Ok(())
}

pub fn page_plain_text(text: &str, context: &PlainTextPagerContext) -> Result<(), PagerError> {
    page_plain_text_with(text, context, &mut std::io::stdout().lock())
}

#[cfg(test)]
mod tests;

use anyhow::Result;
use irosh::Session;
use std::path::{Path, PathBuf};

#[cfg(test)]
use proptest::prelude::*;

use super::transfer::TransferContext;

const ESCAPE_KEYWORDS: &[&str] = &["~?", "~help", "~~", "~.", "~C", "~put", "~get"];
const PROMPT_KEYWORDS: &[&str] = &[
    "put",
    "get",
    "lls",
    "lcd",
    "lpwd",
    "paths",
    "clear",
    "help",
    "exit",
    "disconnect",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionEdit {
    pub line: Vec<u8>,
    pub cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionResult {
    None,
    Applied(CompletionEdit),
    Suggestions(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionMode {
    Escape,
    Prompt,
}

pub async fn complete_line(
    mode: CompletionMode,
    session: &mut Session,
    transfer_context: &TransferContext,
    line: &[u8],
    cursor: usize,
) -> Result<CompletionResult> {
    let text = String::from_utf8_lossy(line).to_string();
    let parsed = parse_line(&text, cursor);

    match classify_completion_target(mode, &parsed) {
        CompletionTarget::Keyword(token) => Ok(complete_keyword(mode, &parsed, token)),
        CompletionTarget::PutLocalPath(raw) | CompletionTarget::ListLocalPath(raw) | CompletionTarget::ChangeLocalPath(raw) => {
            Ok(complete_local_path(&parsed, raw, transfer_context))
        }
        CompletionTarget::GetRemotePath(raw) => complete_remote_path(session, &parsed, raw).await,
        CompletionTarget::None => Ok(CompletionResult::None),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CompletionTarget<'a> {
    Keyword(&'a Token),
    PutLocalPath(&'a str),
    GetRemotePath(&'a str),
    ListLocalPath(&'a str),
    ChangeLocalPath(&'a str),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedLine<'a> {
    raw: &'a str,
    cursor: usize,
    tokens: Vec<Token>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    raw_start: usize,
    raw_end: usize,
    value: String,
}

struct TextEdit {
    line: String,
    cursor: usize,
}

impl TextEdit {
    fn into_bytes(self) -> CompletionEdit {
        CompletionEdit {
            line: self.line.into_bytes(),
            cursor: self.cursor,
        }
    }
}

fn classify_completion_target<'a>(
    mode: CompletionMode,
    parsed: &'a ParsedLine<'a>,
) -> CompletionTarget<'a> {
    let Some(active) = active_token(parsed) else {
        return CompletionTarget::None;
    };

    if active.raw_end < parsed.cursor {
        return CompletionTarget::None;
    }

    let Some(command) = parsed.tokens.first().map(|token| token.value.as_str()) else {
        return CompletionTarget::None;
    };

    if active.raw_start == 0 {
        return CompletionTarget::Keyword(active);
    }

    let mut positional_index = 0usize;
    for token in parsed.tokens.iter().skip(1) {
        if token.raw_start == active.raw_start && token.raw_end == active.raw_end {
            return classify_positional_target(mode, command, positional_index, &token.value);
        }

        if !token.value.starts_with('-') {
            positional_index += 1;
        }
    }

    CompletionTarget::None
}

fn classify_positional_target<'a>(
    mode: CompletionMode,
    command: &str,
    positional_index: usize,
    token: &'a str,
) -> CompletionTarget<'a> {
    match mode {
        CompletionMode::Escape => match command {
            "~put" if positional_index == 0 => CompletionTarget::PutLocalPath(token),
            "~put" if positional_index == 1 => CompletionTarget::GetRemotePath(token),
            "~get" if positional_index == 0 => CompletionTarget::GetRemotePath(token),
            "~get" if positional_index == 1 => CompletionTarget::PutLocalPath(token),
            _ => CompletionTarget::None,
        },
        CompletionMode::Prompt => match command {
            "put" if positional_index == 0 => CompletionTarget::PutLocalPath(token),
            "put" if positional_index == 1 => CompletionTarget::GetRemotePath(token),
            "get" if positional_index == 0 => CompletionTarget::GetRemotePath(token),
            "get" if positional_index == 1 => CompletionTarget::PutLocalPath(token),
            "lls" if positional_index == 0 => CompletionTarget::ListLocalPath(token),
            "lcd" if positional_index == 0 => CompletionTarget::ChangeLocalPath(token),
            _ => CompletionTarget::None,
        },
    }
}

fn complete_keyword(
    mode: CompletionMode,
    parsed: &ParsedLine<'_>,
    token: &Token,
) -> CompletionResult {
    let keywords = match mode {
        CompletionMode::Escape => ESCAPE_KEYWORDS,
        CompletionMode::Prompt => PROMPT_KEYWORDS,
    };

    let matches: Vec<&str> = keywords
        .iter()
        .copied()
        .filter(|candidate| candidate.starts_with(&token.value))
        .collect();

    match matches.as_slice() {
        [] => CompletionResult::None,
        [single] => {
            let replacement = match *single {
                "~put" | "~get" | "put" | "get" | "lls" | "lcd" => format!("{single} "),
                other => other.to_string(),
            };

            CompletionResult::Applied(
                replace_range(parsed, token.raw_start, token.raw_end, &replacement).into_bytes(),
            )
        }
        matches => {
            CompletionResult::Suggestions(matches.iter().map(std::string::ToString::to_string).collect())
        }
    }
}

fn complete_local_path(
    parsed: &ParsedLine<'_>,
    raw: &str,
    transfer_context: &TransferContext,
) -> CompletionResult {
    let resolved = transfer_context.resolve_local_source(raw);
    let (search_dir, prefix, base) = local_completion_parts(raw, &resolved);

    let Ok(entries) = std::fs::read_dir(&search_dir) else {
        return CompletionResult::None;
    };

    let mut matches = entries
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with(&prefix) {
                return None;
            }

            let is_dir = entry.file_type().ok().is_some_and(|kind| kind.is_dir());
            Some((name, is_dir))
        })
        .collect::<Vec<_>>();

    matches.sort_by(|left, right| left.0.cmp(&right.0));
    match matches.as_slice() {
        [] => CompletionResult::None,
        [(name, is_dir)] => {
            let mut completed = format!("{base}{name}");
            let replacement = if *is_dir {
                completed.push('/');
                shell_words::quote(&completed).into_owned()
            } else {
                format!("{} ", shell_words::quote(&completed))
            };

            let Some(token) = active_token(parsed) else {
                return CompletionResult::None;
            };

            CompletionResult::Applied(
                replace_range(parsed, token.raw_start, token.raw_end, &replacement).into_bytes(),
            )
        }
        many => CompletionResult::Suggestions(
            many.iter()
                .map(|(name, is_dir)| {
                    if *is_dir {
                        format!("{base}{name}/")
                    } else {
                        format!("{base}{name}")
                    }
                })
                .collect(),
        ),
    }
}

async fn complete_remote_path(
    session: &mut Session,
    parsed: &ParsedLine<'_>,
    raw: &str,
) -> Result<CompletionResult> {
    let mut matches = session.remote_completion(raw).await?;
    matches.sort();

    match matches.as_slice() {
        [] => Ok(CompletionResult::None),
        [single] => {
            let replacement = if single.ends_with('/') {
                single.clone()
            } else {
                format!("{single} ")
            };

            let Some(token) = active_token(parsed) else {
                return Ok(CompletionResult::None);
            };

            Ok(CompletionResult::Applied(
                replace_range(parsed, token.raw_start, token.raw_end, &replacement).into_bytes(),
            ))
        }
        many => Ok(CompletionResult::Suggestions(many.to_vec())),
    }
}

fn replace_range(parsed: &ParsedLine<'_>, start: usize, end: usize, replacement: &str) -> TextEdit {
    let mut line = String::with_capacity(parsed.raw.len() + replacement.len());
    line.push_str(&parsed.raw[..start]);
    line.push_str(replacement);
    line.push_str(&parsed.raw[end..]);

    TextEdit {
        line,
        cursor: start + replacement.len(),
    }
}

fn active_token<'a>(parsed: &'a ParsedLine<'a>) -> Option<&'a Token> {
    parsed
        .tokens
        .iter()
        .find(|token| parsed.cursor >= token.raw_start && parsed.cursor <= token.raw_end)
}

fn local_completion_parts(raw: &str, resolved: &Path) -> (PathBuf, String, String) {
    if raw.is_empty() {
        return (resolved.to_path_buf(), String::new(), String::new());
    }

    if raw.ends_with('/') {
        return (resolved.to_path_buf(), String::new(), raw.to_string());
    }

    let prefix = resolved
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("")
        .to_string();

    let search_dir = resolved
        .parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    let base = match raw.rfind('/') {
        Some(index) => raw[..=index].to_string(),
        None => String::new(),
    };

    (search_dir, prefix, base)
}

fn parse_line(raw: &str, cursor: usize) -> ParsedLine<'_> {
    let mut cursor = cursor.min(raw.len());
    // Ensure cursor is at a valid char boundary
    while cursor > 0 && !raw.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let cursor = cursor;
    let mut tokens = Vec::new();

    let mut token_start = None;
    let mut token_value = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    let mut finish_token =
        |end: usize, token_start: &mut Option<usize>, token_value: &mut String| {
            if let Some(start) = token_start.take() {
                tokens.push(Token {
                    raw_start: start,
                    raw_end: end,
                    value: std::mem::take(token_value),
                });
            }
        };

    for (index, ch) in raw.char_indices() {
        if escaped {
            if token_start.is_none() {
                token_start = Some(index);
            }
            token_value.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single => {
                if token_start.is_none() {
                    token_start = Some(index);
                }
                escaped = true;
            }
            '\'' if !in_double => {
                if token_start.is_none() {
                    token_start = Some(index);
                }
                in_single = !in_single;
            }
            '"' if !in_single => {
                if token_start.is_none() {
                    token_start = Some(index);
                }
                in_double = !in_double;
            }
            ch if ch.is_whitespace() && !in_single && !in_double => {
                finish_token(index, &mut token_start, &mut token_value);
            }
            _ => {
                if token_start.is_none() {
                    token_start = Some(index);
                }
                token_value.push(ch);
            }
        }
    }

    finish_token(raw.len(), &mut token_start, &mut token_value);

    if raw[..cursor]
        .chars()
        .last()
        .is_some_and(char::is_whitespace)
    {
        tokens.push(Token {
            raw_start: cursor,
            raw_end: cursor,
            value: String::new(),
        });
    }

    ParsedLine {
        raw,
        cursor,
        tokens,
    }
}

#[cfg(test)]
proptest! {
    /// Bruteforce test: Ensure that NO input string or cursor position can cause the tokenizer to panic.
    #[test]
    fn fuzz_completion_tokenizer(raw in ".*", cursor in 0usize..1000) {
        let _ = parse_line(&raw, cursor);
    }
}

use std::path::{Path, PathBuf};

use anyhow::Result;
use irosh::Session;

use crate::commands::connect::transfer::TransferContext;

const ESCAPE_KEYWORDS: &[&str] = &["~?", "~help", "~~", "~.", "~C", "~put", "~get"];
const PROMPT_KEYWORDS: &[&str] = &[
    "put",
    "get",
    "ls",
    "cd",
    "pwd",
    "paths",
    "help",
    "?",
    "exit",
    "disconnect",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CompletionEdit {
    pub(super) line: Vec<u8>,
    pub(super) cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CompletionResult {
    None,
    Applied(CompletionEdit),
    Suggestions(Vec<String>),
}

pub(super) async fn complete_escape_line(
    session: &mut Session,
    transfer_context: &TransferContext,
    line: &[u8],
    cursor: usize,
) -> Result<CompletionResult> {
    complete_line(
        CompletionMode::Escape,
        session,
        transfer_context,
        line,
        cursor,
    )
    .await
}

pub(super) async fn complete_prompt_line(
    session: &mut Session,
    transfer_context: &TransferContext,
    line: &[u8],
    cursor: usize,
) -> Result<CompletionResult> {
    complete_line(
        CompletionMode::Prompt,
        session,
        transfer_context,
        line,
        cursor,
    )
    .await
}

async fn complete_line(
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
        CompletionTarget::PutLocalPath(raw) => complete_local_path(&parsed, raw, transfer_context),
        CompletionTarget::GetRemotePath(raw) => complete_remote_path(session, &parsed, raw).await,
        CompletionTarget::ListLocalPath(raw) => complete_local_path(&parsed, raw, transfer_context),
        CompletionTarget::ChangeLocalPath(raw) => {
            complete_local_path(&parsed, raw, transfer_context)
        }
        CompletionTarget::None => Ok(CompletionResult::None),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionMode {
    Escape,
    Prompt,
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
            "ls" if positional_index == 0 => CompletionTarget::ListLocalPath(token),
            "cd" if positional_index == 0 => CompletionTarget::ChangeLocalPath(token),
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
        [matched] => {
            let replacement = match *matched {
                "~put" | "~get" | "put" | "get" | "ls" | "cd" => format!("{matched} "),
                other => other.to_string(),
            };

            CompletionResult::Applied(
                replace_range(parsed, token.raw_start, token.raw_end, &replacement).into_bytes(),
            )
        }
        matches => {
            CompletionResult::Suggestions(matches.iter().map(|entry| entry.to_string()).collect())
        }
    }
}

fn complete_local_path(
    parsed: &ParsedLine<'_>,
    raw: &str,
    transfer_context: &TransferContext,
) -> Result<CompletionResult> {
    let resolved = transfer_context.resolve_local_source(raw);
    let (search_dir, prefix, base) = local_completion_parts(raw, &resolved);

    let Ok(entries) = std::fs::read_dir(&search_dir) else {
        return Ok(CompletionResult::None);
    };

    let mut matches = entries
        .filter_map(|entry| entry.ok())
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
        [] => Ok(CompletionResult::None),
        [(name, is_dir)] => {
            let mut completed = format!("{base}{name}");
            let replacement = if *is_dir {
                completed.push('/');
                shell_words::quote(&completed).into_owned()
            } else {
                format!("{} ", shell_words::quote(&completed))
            };

            let Some(token) = active_token(parsed) else {
                return Ok(CompletionResult::None);
            };

            Ok(CompletionResult::Applied(
                replace_range(parsed, token.raw_start, token.raw_end, &replacement).into_bytes(),
            ))
        }
        many => Ok(CompletionResult::Suggestions(
            many.iter()
                .map(|(name, is_dir)| {
                    if *is_dir {
                        format!("{base}{name}/")
                    } else {
                        format!("{base}{name}")
                    }
                })
                .collect(),
        )),
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
        [matched] => {
            let replacement = if matched.ends_with('/') {
                matched.clone()
            } else {
                format!("{matched} ")
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
        return (PathBuf::from("."), String::new(), String::new());
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
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let base = match raw.rfind('/') {
        Some(index) => raw[..=index].to_string(),
        None => String::new(),
    };

    (search_dir, prefix, base)
}

fn parse_line(raw: &str, cursor: usize) -> ParsedLine<'_> {
    let cursor = cursor.min(raw.len());
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
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        CompletionMode, CompletionResult, CompletionTarget, classify_completion_target,
        complete_keyword, local_completion_parts, parse_line,
    };
    use crate::commands::connect::transfer::TransferContext;

    #[test]
    fn parse_line_tracks_empty_token_after_space() {
        let parsed = parse_line("~put src/ ", 10);
        assert_eq!(parsed.tokens.len(), 3);
        assert_eq!(parsed.tokens[2].raw_start, 10);
        assert_eq!(parsed.tokens[2].value, "");
    }

    #[test]
    fn classify_keyword_target_for_partial_command() {
        let parsed = parse_line("~he", 3);
        assert!(matches!(
            classify_completion_target(CompletionMode::Escape, &parsed),
            CompletionTarget::Keyword(token) if token.value == "~he"
        ));
    }

    #[test]
    fn classify_put_local_path_after_flags() {
        let parsed = parse_line("~put -r src/ma", 14);
        assert!(matches!(
            classify_completion_target(CompletionMode::Escape, &parsed),
            CompletionTarget::PutLocalPath("src/ma")
        ));
    }

    #[test]
    fn classify_get_remote_path_after_flags() {
        let parsed = parse_line("~get --recursive /va", 21);
        assert!(matches!(
            classify_completion_target(CompletionMode::Escape, &parsed),
            CompletionTarget::GetRemotePath("/va")
        ));
    }

    #[test]
    fn classify_put_remote_target_as_remote_path() {
        let parsed = parse_line("~put local.txt rem", 18);
        assert!(matches!(
            classify_completion_target(CompletionMode::Escape, &parsed),
            CompletionTarget::GetRemotePath("rem")
        ));
    }

    #[test]
    fn classify_get_local_target_as_local_path() {
        let parsed = parse_line("~get remote.txt loc", 19);
        assert!(matches!(
            classify_completion_target(CompletionMode::Escape, &parsed),
            CompletionTarget::PutLocalPath("loc")
        ));
    }

    #[test]
    fn classify_prompt_cd_as_local_path() {
        let parsed = parse_line("cd no", 5);
        assert!(matches!(
            classify_completion_target(CompletionMode::Prompt, &parsed),
            CompletionTarget::ChangeLocalPath("no")
        ));
    }

    #[test]
    fn complete_keyword_appends_space_for_transfer_commands() {
        let parsed = parse_line("~pu", 3);
        let token = &parsed.tokens[0];
        let CompletionResult::Applied(edit) =
            complete_keyword(CompletionMode::Escape, &parsed, token)
        else {
            panic!("expected unique completion");
        };
        assert_eq!(String::from_utf8(edit.line).expect("utf8"), "~put ");
        assert_eq!(edit.cursor, 5);
    }

    #[test]
    fn complete_keyword_lists_multiple_matches() {
        let parsed = parse_line("~", 1);
        let token = &parsed.tokens[0];
        let CompletionResult::Suggestions(matches) =
            complete_keyword(CompletionMode::Escape, &parsed, token)
        else {
            panic!("expected suggestions");
        };
        assert!(matches.contains(&"~help".to_string()));
        assert!(matches.contains(&"~put".to_string()));
        assert!(matches.contains(&"~C".to_string()));
    }

    #[test]
    fn prompt_keyword_completion_appends_space() {
        let parsed = parse_line("pw", 2);
        let token = &parsed.tokens[0];
        let CompletionResult::Applied(edit) =
            complete_keyword(CompletionMode::Prompt, &parsed, token)
        else {
            panic!("expected prompt completion");
        };
        assert_eq!(String::from_utf8(edit.line).expect("utf8"), "pwd");
    }

    #[test]
    fn local_completion_parts_preserve_base_prefix() {
        let resolved = PathBuf::from("/tmp/demo/alpha");
        let (search_dir, prefix, base) = local_completion_parts("demo/al", &resolved);
        assert_eq!(search_dir, PathBuf::from("/tmp/demo"));
        assert_eq!(prefix, "alpha");
        assert_eq!(base, "demo/");
    }

    #[test]
    fn local_path_completion_quotes_spaces() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("irosh-complete-{unique}"));
        std::fs::create_dir_all(&root).expect("create temp completion dir");
        let file = root.join("space name.txt");
        std::fs::write(&file, b"data").expect("create completion file");

        let transfer_context = TransferContext {
            local_root: root.clone(),
        };
        let parsed = parse_line("~put spa", 8);
        let CompletionTarget::PutLocalPath(raw) =
            classify_completion_target(CompletionMode::Escape, &parsed)
        else {
            panic!("expected local path completion target");
        };

        let CompletionResult::Applied(edit) =
            super::complete_local_path(&parsed, raw, &transfer_context)
                .expect("completion should succeed")
        else {
            panic!("expected unique completion");
        };

        assert_eq!(
            String::from_utf8(edit.line).expect("utf8"),
            "~put 'space name.txt' "
        );

        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_dir(root);
    }
}

use anyhow::Result;
use tokio::io::AsyncWriteExt;

pub(crate) async fn print_local_block(stdout: &mut tokio::io::Stdout, body: &str) -> Result<()> {
    stdout.write_all(b"\r\n").await?;
    write_crlf_text(stdout, body).await?;
    stdout.flush().await?;
    Ok(())
}

pub(super) async fn print_completion_suggestions(
    stdout: &mut tokio::io::Stdout,
    suggestions: &[String],
) -> Result<()> {
    if suggestions.is_empty() {
        return Ok(());
    }

    let body = format!("{}\n", suggestions.join("  "));
    print_local_block(stdout, &body).await
}

pub(crate) async fn print_escape_help(stdout: &mut tokio::io::Stdout) -> Result<()> {
    let help = r#"
Supported escape sequences:
   ~? (~help) - this message
   ~.         - terminate connection
   ~~         - send the escape character by typing it twice
   ~C         - open the irosh> local command prompt
   ~put [-r]  - upload a file or directory to the remote
   ~get [-r]  - download a file or directory from the remote

(Note that escapes are only recognized immediately after newline.)
"#;

    print_local_block(stdout, help.trim_start().trim_end()).await
}

pub(crate) async fn write_crlf_text(stdout: &mut tokio::io::Stdout, text: &str) -> Result<()> {
    let formatted = text.replace('\n', "\r\n");
    stdout.write_all(formatted.as_bytes()).await?;
    Ok(())
}

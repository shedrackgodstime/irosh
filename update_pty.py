import sys
with open('src/server/handler/pty.rs', 'r', encoding='utf-8') as f:
    content = f.read()

content = content.replace('writer: Option<Box<dyn Write + Send>>', 'pty_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>')
content = content.replace('writer: Some(writer)', 'pty_tx: Some(pty_tx)')
content = content.replace('process.writer.as_mut()', 'process.pty_tx.as_ref()')
content = content.replace('let _ = writer.write_all(data);\n            let _ = writer.flush();', 'let _ = pty_tx.send(data.to_vec());')
content = content.replace('let _ = writer.write_all(data);\r\n            let _ = writer.flush();', 'let _ = pty_tx.send(data.to_vec());')
content = content.replace('process.writer.take()', 'process.pty_tx.take()')

take_writer = '''        let mut writer = pair
            .master
            .take_writer()
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to take PTY writer: {e}"),
            })?;'''

new_take_writer = '''        let mut writer = pair
            .master
            .take_writer()
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to take PTY writer: {e}"),
            })?;

        let (pty_tx, mut pty_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let writer_done = shutdown.clone();

        tokio::task::spawn_blocking(move || {
            while let Some(data) = pty_rx.blocking_recv() {
                if writer_done.is_cancelled() {
                    break;
                }
                if let Err(e) = writer.write_all(&data) {
                    warn!("Failed to write to PTY: {:?}", e);
                    break;
                }
                let _ = writer.flush();
            }
        });'''
content = content.replace('        let writer = pair\n            .master\n            .take_writer()', '        let mut writer = pair\n            .master\n            .take_writer()')
content = content.replace('        let writer = pair\r\n            .master\r\n            .take_writer()', '        let mut writer = pair\r\n            .master\r\n            .take_writer()')

content = content.replace(take_writer, new_take_writer)
content = content.replace(take_writer.replace('\n', '\r\n'), new_take_writer.replace('\n', '\r\n'))

with open('src/server/handler/pty.rs', 'w', encoding='utf-8') as f:
    f.write(content)

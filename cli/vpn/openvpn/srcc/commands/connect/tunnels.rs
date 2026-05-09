use crate::ui::Ui;
use anyhow::Result;
use irosh::Session;

pub async fn setup_forwarding(session: &mut Session, forward_str: Option<String>) -> Result<()> {
    if let Some(f) = forward_str {
        let parts: Vec<&str> = f.split(':').collect();
        if parts.len() == 3 {
            let local_port: u16 = parts[0].parse()?;
            let remote_host = parts[1].to_string();
            let remote_port: u32 = parts[2].parse()?;
            let local_addr = format!("127.0.0.1:{}", local_port);
            match session
                .local_forward(&local_addr, remote_host.clone(), remote_port)
                .await
            {
                Ok((_handle, bound)) => Ui::info(&format!(
                    "Forwarding {} -> {}:{}",
                    bound, remote_host, remote_port
                )),
                Err(e) => Ui::error(&format!("Forwarding failed: {}", e)),
            }
        }
    }
    Ok(())
}

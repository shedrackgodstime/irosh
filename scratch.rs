use std::path::PathBuf;
#[tokio::main]
async fn main() {
    let root = dirs::home_dir().unwrap().join(".irosh").join("server");
    let client = irosh::IpcClient::new(root);
    match client.send(irosh::IpcCommand::GetStatus).await {
        Ok(res) => println!("Success: {:?}", res),
        Err(e) => println!("Error: {}", e),
    }
}

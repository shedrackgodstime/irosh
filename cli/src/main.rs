// irosh-cli (Thin CLI)
// All UI prompts, progress bars, and terminal interaction go here.
// DO NOT put core networking or state management in this crate.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Irosh V2 Migration Started!");
    Ok(())
}

use irosh::config::HostKeyPolicy;
use irosh::{Client, ClientOptions, SecurityConfig, Server, ServerOptions, StateConfig};
use std::time::Duration;

fn temp_state(name: &str) -> StateConfig {
    let mut path = std::env::temp_dir();
    path.push(format!("irosh-verify-{}-{}", name, rand::random::<u32>()));
    StateConfig::new(path)
}

#[tokio::test]
async fn verify_exec_output() {
    let server_state = temp_state("server-verify");
    let client_state = temp_state("client-verify");

    let server_opts = ServerOptions::new(server_state.clone()).security(SecurityConfig {
        host_key_policy: HostKeyPolicy::AcceptAll,
    });

    let (ready, server) = Server::bind(server_opts).await.unwrap();
    let ticket = ready.ticket().clone();
    let shutdown = server.shutdown_handle();
    let server_handle = tokio::spawn(async move { server.run().await });

    tokio::time::sleep(Duration::from_millis(500)).await;

    let client_opts = ClientOptions::new(client_state.clone()).security(SecurityConfig {
        host_key_policy: HostKeyPolicy::AcceptAll,
    });
    let mut session = Client::connect(&client_opts, ticket).await.unwrap();

    println!("\n--- CAPTURE EXEC DEBUG ---");
    let output = session.capture_exec("echo 'TEST_MARKER'").await.unwrap();
    println!("STDOUT BYTES: {:?}", output.stdout);
    println!(
        "STDOUT STRING: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    println!("STDERR BYTES: {:?}", output.stderr);
    println!(
        "STDERR STRING: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    println!("EXIT STATUS: {}", output.exit_status);

    println!("\n--- DIRECTORY CHECK DEBUG ---");
    // Check existing directory (server_state root)
    let dir_path = server_state.root().display().to_string();
    let cmd = format!(
        "if [ -d \"{}\" ]; then echo 'YES'; else echo 'NO'; fi",
        dir_path
    );
    let output = session.capture_exec(&cmd).await.unwrap();
    println!(
        "DIR CHECK STDOUT: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );

    session.close().await.unwrap();
    shutdown.close().await;
    let _ = server_handle.await;
    let _ = tokio::fs::remove_dir_all(server_state.root()).await;
    let _ = tokio::fs::remove_dir_all(client_state.root()).await;
}

use super::*;

#[tokio::test]
async fn session_state_transitions_from_authenticated_to_shell_ready_to_closed() {
    let server_state = temp_state_dir("server-session-state");
    let client_state = temp_state_dir("client-session-state");
    let (mut session, server_task) = connect_test_session(&server_state, &client_state).await;

    assert_eq!(session.state(), SessionState::Authenticated);
    session.start_shell().await.unwrap();
    assert_eq!(session.state(), SessionState::ShellReady);
    session.disconnect().await.unwrap();
    assert_eq!(session.state(), SessionState::Closed);

    let _ = server_task.await.unwrap();
}

#[tokio::test]
async fn exec_emits_stdout_and_close_events() {
    let server_state = temp_state_dir("server-exec");
    let client_state = temp_state_dir("client-exec");
    let (mut session, server_task) = connect_test_session(&server_state, &client_state).await;

    session.exec("printf exec-ok").await.unwrap();

    let mut stdout = Vec::new();
    loop {
        let Some(event) = session.next_event().await.unwrap() else {
            break;
        };
        match event {
            SessionEvent::Stdout(data) => stdout.extend_from_slice(&data),
            SessionEvent::Closed => break,
            SessionEvent::Stderr(_) | SessionEvent::ExitStatus(_) => {}
        }
    }

    let stdout = String::from_utf8(stdout).unwrap();
    assert!(stdout.contains("exec-ok"), "unexpected stdout: {stdout}");
    assert_eq!(session.state(), SessionState::Closed);

    let _ = server_task.await.unwrap();
}

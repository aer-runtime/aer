use aer_core::{AerError, Event, Task};

/// Returns a shell invocation that exits with the given code, cross-platform.
fn exit_cmd(code: i32) -> (String, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        ("cmd".into(), vec!["/c".into(), format!("exit {}", code)])
    }
    #[cfg(not(target_os = "windows"))]
    {
        ("sh".into(), vec!["-c".into(), format!("exit {}", code)])
    }
}

/// Returns a shell invocation that writes N lines to stdout.
fn noisy_cmd(lines: usize) -> (String, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        (
            "cmd".into(),
            vec![
                "/c".into(),
                format!("for /L %i in (1,1,{}) do @echo line %i", lines),
            ],
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        (
            "sh".into(),
            vec![
                "-c".into(),
                format!("seq 1 {} | while read i; do echo \"line $i\"; done", lines),
            ],
        )
    }
}

fn collect_events(program: &str, args: Vec<String>) -> (Result<(), AerError>, Vec<Event>) {
    let task = Task::new(program, args);
    let mut events = Vec::new();
    let result = task.run(|e| events.push(e));
    (result, events)
}

// --- Tests ---

#[test]
fn exit_zero_emits_started_then_exited() {
    let (prog, args) = exit_cmd(0);
    let (result, events) = collect_events(&prog, args);

    assert!(result.is_ok());
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], Event::Started { pid } if pid > 0));
    assert!(matches!(events[1], Event::Exited { code: 0 }));
}

#[test]
fn nonzero_exit_code_is_captured() {
    let (prog, args) = exit_cmd(42);
    let (result, events) = collect_events(&prog, args);

    assert!(result.is_ok());
    assert!(matches!(events[1], Event::Exited { code: 42 }));
}

#[test]
fn started_always_precedes_exited() {
    let (prog, args) = exit_cmd(0);
    let (_, events) = collect_events(&prog, args);

    let started_pos = events
        .iter()
        .position(|e| matches!(e, Event::Started { .. }));
    let exited_pos = events
        .iter()
        .position(|e| matches!(e, Event::Exited { .. }));

    assert!(started_pos.is_some() && exited_pos.is_some());
    assert!(started_pos.unwrap() < exited_pos.unwrap());
}

#[test]
fn spawn_failure_returns_error_and_emits_no_events() {
    let (result, events) = collect_events("definitely_not_a_real_binary_xyzzy_aer", vec![]);

    assert!(matches!(result, Err(AerError::SpawnFailed(_))));
    assert!(
        events.is_empty(),
        "no events should be emitted when spawn fails"
    );
}

#[test]
fn large_output_does_not_deadlock() {
    // Verifies that wait_with_output() drains the pipe even for large output,
    // preventing the OS pipe-buffer deadlock. 1000 lines well exceeds typical
    // 64 KB pipe buffers when each line is ~10 bytes.
    let (prog, args) = noisy_cmd(1000);
    let (result, events) = collect_events(&prog, args);

    assert!(
        result.is_ok(),
        "large output caused deadlock or error: {:?}",
        result
    );
    assert!(matches!(events[1], Event::Exited { code: 0 }));
}

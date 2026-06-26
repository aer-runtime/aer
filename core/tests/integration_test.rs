use aer_core::{AerError, Event, Task};
use std::fs;
use std::thread;
use std::time::Duration;

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

/// Returns a shell invocation that sleeps for N seconds, cross-platform.
///
/// On Windows, `timeout /t` exits immediately when stdin is not a console
/// (which it isn't when spawned with piped stdio). `ping -n (secs+1)` is the
/// standard workaround: each ping round-trip to 127.0.0.1 takes ~1 second.
fn sleep_cmd(secs: u64) -> (String, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        (
            "ping".into(),
            vec!["-n".into(), format!("{}", secs + 1), "127.0.0.1".into()],
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        ("sh".into(), vec!["-c".into(), format!("sleep {}", secs)])
    }
}

fn collect_events(program: &str, args: Vec<String>) -> (Result<(), AerError>, Vec<Event>) {
    let task = Task::new(program, args);
    let mut events = Vec::new();
    let result = task.run(|e| events.push(e));
    (result, events)
}

fn collect_events_with_timeout(
    program: &str,
    args: Vec<String>,
    timeout: Duration,
) -> (Result<(), AerError>, Vec<Event>) {
    let task = Task::new(program, args).with_timeout(timeout);
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

// --- M2: Timeout & Kill Escalation ---

#[test]
fn timeout_kills_slow_process() {
    let (prog, args) = sleep_cmd(60);
    let (result, events) = collect_events_with_timeout(&prog, args, Duration::from_secs(1));

    assert!(
        matches!(result, Err(AerError::TimedOut)),
        "expected TimedOut, got {:?}",
        result
    );
    assert_eq!(events.len(), 2, "expected Started + Exited even on timeout");
    assert!(matches!(events[0], Event::Started { pid } if pid > 0));
    assert!(matches!(events[1], Event::Exited { code: -1 }));
}

#[test]
fn no_timeout_set_runs_normally() {
    // Regression guard: Task::new() without with_timeout behaves identically to M1.
    let (prog, args) = exit_cmd(0);
    let (result, events) = collect_events(&prog, args);

    assert!(result.is_ok());
    assert_eq!(events.len(), 2);
    assert!(matches!(events[1], Event::Exited { code: 0 }));
}

#[test]
fn timeout_does_not_fire_for_fast_process() {
    // Process exits before the timeout — run() should return Ok, not TimedOut.
    let (prog, args) = exit_cmd(0);
    let (result, events) = collect_events_with_timeout(&prog, args, Duration::from_secs(30));

    assert!(
        result.is_ok(),
        "expected Ok for fast process with long timeout, got {:?}",
        result
    );
    assert!(matches!(events[1], Event::Exited { code: 0 }));
}

// --- M3: Process Tree Cleanup ---

/// Returns a command that spawns a long-lived grandchild, writes its PID to
/// `pid_file`, then exits immediately. Used to verify process tree cleanup.
fn orphan_cmd(pid_file: &str) -> (String, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        // Start-Process spawns ping as a child of PowerShell. PowerShell writes
        // ping's PID to the file, then exits. With M3, the job object kills ping
        // when PowerShell exits and the job handle is closed.
        (
            "powershell".into(),
            vec![
                "-NoProfile".into(),
                "-Command".into(),
                format!(
                    "$p = Start-Process -PassThru -NoNewWindow -FilePath ping \
                     -ArgumentList @('-n','9999','127.0.0.1'); \
                     $p.Id | Out-File -FilePath '{}' -Encoding ascii",
                    pid_file
                ),
            ],
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Background sleep inherits the session but not the process group after
        // setsid. $! captures its PID for verification.
        (
            "sh".into(),
            vec!["-c".into(), format!("sleep 9999 & echo $! > {pid_file}")],
        )
    }
}

/// Returns true if the process with the given PID is still running.
#[cfg(not(target_os = "windows"))]
fn process_is_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Returns true if the process with the given PID is still running.
#[cfg(target_os = "windows")]
fn process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let mut code: u32 = 0;
    let ok = unsafe { GetExitCodeProcess(handle, &mut code) };
    unsafe { CloseHandle(handle) };
    ok != 0 && code == STILL_ACTIVE as u32
}

#[test]
fn process_tree_is_cleaned_up() {
    let pid_file = std::env::temp_dir().join("aer_test_grandchild_pid.txt");
    let pid_file_str = pid_file.to_str().unwrap();

    let (prog, args) = orphan_cmd(pid_file_str);
    let task = Task::new(prog, args);
    let start = std::time::Instant::now();
    let result = task.run(|_| {});
    let elapsed = start.elapsed();
    assert!(result.is_ok(), "task failed unexpectedly: {:?}", result);
    assert!(
        elapsed < Duration::from_secs(10),
        "run() took {elapsed:?} — process tree cleanup is deadlocked (grandchild holds the pipe)"
    );

    // Brief pause: job object cleanup (Windows) or pipe-close propagation (Unix)
    // is synchronous within run(), but give the OS a moment to complete.
    thread::sleep(Duration::from_millis(200));

    let raw = fs::read_to_string(&pid_file)
        .expect("grandchild PID file not written — orphan_cmd may have failed");
    let grandchild_pid: u32 = raw
        .trim()
        .parse()
        .expect("grandchild PID file contains non-numeric data");

    assert!(
        !process_is_alive(grandchild_pid),
        "grandchild PID {grandchild_pid} is still alive — process tree cleanup failed"
    );

    let _ = fs::remove_file(&pid_file);
}

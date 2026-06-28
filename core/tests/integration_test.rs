use aer_core::{
    ffi::{self, AerErrorCode, AerEvent},
    AerError, CancelHandle, Event, ExitReason, Task,
};
use std::ffi::{c_void, CStr, CString};
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

/// Returns a shell invocation that writes `msg` to stderr then exits 0.
fn stderr_cmd(msg: &str) -> (String, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        (
            "cmd".into(),
            vec!["/c".into(), format!("echo {} 1>&2", msg)],
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        ("sh".into(), vec!["-c".into(), format!("echo {} >&2", msg)])
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
    assert!(matches!(events[1], Event::Exited { code: 0, .. }));
}

#[test]
fn nonzero_exit_code_is_captured() {
    let (prog, args) = exit_cmd(42);
    let (result, events) = collect_events(&prog, args);

    assert!(result.is_ok());
    assert!(matches!(events[1], Event::Exited { code: 42, .. }));
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
    assert!(matches!(events[1], Event::Exited { code: 0, .. }));
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
    assert!(matches!(events[1], Event::Exited { code: -1, .. }));
}

#[test]
fn no_timeout_set_runs_normally() {
    // Regression guard: Task::new() without with_timeout behaves identically to M1.
    let (prog, args) = exit_cmd(0);
    let (result, events) = collect_events(&prog, args);

    assert!(result.is_ok());
    assert_eq!(events.len(), 2);
    assert!(matches!(events[1], Event::Exited { code: 0, .. }));
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
    assert!(matches!(events[1], Event::Exited { code: 0, .. }));
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

// --- M4: FFI Boundary ---

// Closures cannot be extern "C". State is passed through user_data instead.
unsafe extern "C" fn collect_ffi_events(event: *const AerEvent, user_data: *mut c_void) {
    let events = &mut *(user_data as *mut Vec<AerEvent>);
    events.push(*event);
}

/// Build CStrings from (program, args) and call aer_task_new.
/// Returns the task pointer; the CStrings must remain alive for the duration of the call.
fn ffi_new_task(program: &str, args: &[String]) -> *mut ffi::AerTask {
    let c_program = CString::new(program).unwrap();
    let c_args: Vec<CString> = args
        .iter()
        .map(|s| CString::new(s.as_str()).unwrap())
        .collect();
    let arg_ptrs: Vec<*const i8> = c_args.iter().map(|s| s.as_ptr()).collect();
    unsafe {
        ffi::aer_task_new(
            c_program.as_ptr(),
            if arg_ptrs.is_empty() {
                std::ptr::null()
            } else {
                arg_ptrs.as_ptr()
            },
            arg_ptrs.len(),
        )
    }
}

#[test]
fn ffi_null_program_returns_null() {
    let task = unsafe { ffi::aer_task_new(std::ptr::null(), std::ptr::null(), 0) };
    assert!(task.is_null(), "expected NULL for null program");
}

#[test]
fn ffi_basic_run_emits_events() {
    let (prog, args) = exit_cmd(0);
    let task = ffi_new_task(&prog, &args);
    assert!(!task.is_null());

    let mut events: Vec<AerEvent> = Vec::new();
    let code = unsafe {
        ffi::aer_task_run(
            task,
            Some(collect_ffi_events),
            &mut events as *mut _ as *mut c_void,
        )
    };
    unsafe { ffi::aer_task_free(task) };

    assert_eq!(code, AerErrorCode::Ok);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].kind, 0, "first event should be Started");
    assert!(events[0].pid > 0, "started pid should be > 0");
    assert_eq!(events[1].kind, 1, "second event should be Exited");
    assert_eq!(events[1].code, 0);
}

#[test]
fn ffi_null_callback_is_valid() {
    let (prog, args) = exit_cmd(0);
    let task = ffi_new_task(&prog, &args);
    assert!(!task.is_null());

    let code = unsafe { ffi::aer_task_run(task, None, std::ptr::null_mut()) };
    unsafe { ffi::aer_task_free(task) };

    assert_eq!(code, AerErrorCode::Ok);
}

#[test]
fn ffi_double_run_returns_already_run() {
    let (prog, args) = exit_cmd(0);
    let task = ffi_new_task(&prog, &args);
    assert!(!task.is_null());

    let first = unsafe { ffi::aer_task_run(task, None, std::ptr::null_mut()) };
    assert_eq!(first, AerErrorCode::Ok);

    let second = unsafe { ffi::aer_task_run(task, None, std::ptr::null_mut()) };
    assert_eq!(second, AerErrorCode::AlreadyRun);

    unsafe { ffi::aer_task_free(task) };
}

#[test]
fn ffi_null_task_run_returns_null_pointer() {
    let code = unsafe { ffi::aer_task_run(std::ptr::null_mut(), None, std::ptr::null_mut()) };
    assert_eq!(code, AerErrorCode::NullPointer);
}

#[test]
fn ffi_timeout_fires() {
    let (prog, args) = sleep_cmd(60);
    let task = ffi_new_task(&prog, &args);
    assert!(!task.is_null());

    let to_code = unsafe { ffi::aer_task_with_timeout(task, 1000) };
    assert_eq!(to_code, AerErrorCode::Ok);

    let mut events: Vec<AerEvent> = Vec::new();
    let run_code = unsafe {
        ffi::aer_task_run(
            task,
            Some(collect_ffi_events),
            &mut events as *mut _ as *mut c_void,
        )
    };
    unsafe { ffi::aer_task_free(task) };

    assert_eq!(run_code, AerErrorCode::TimedOut);
    assert_eq!(events.len(), 2, "expect Started + Exited even on timeout");
    assert_eq!(events[1].code, -1, "timed-out exit code must be -1");
}

#[test]
fn ffi_spawn_failure_sets_error_message() {
    let task = ffi_new_task("definitely_not_a_real_binary_xyzzy_aer", &[]);
    assert!(!task.is_null());

    let code = unsafe { ffi::aer_task_run(task, None, std::ptr::null_mut()) };
    assert_eq!(code, AerErrorCode::SpawnFailed);

    let msg_ptr = ffi::aer_last_error_message();
    assert!(
        !msg_ptr.is_null(),
        "expected error message for spawn failure"
    );
    let msg = unsafe { CStr::from_ptr(msg_ptr) }.to_str().unwrap();
    assert!(!msg.is_empty(), "error message should not be empty");

    unsafe { ffi::aer_task_free(task) };
}

#[test]
fn ffi_free_null_is_noop() {
    unsafe { ffi::aer_task_free(std::ptr::null_mut()) };
}

#[test]
fn ffi_last_error_message_is_null_after_success() {
    let (prog, args) = exit_cmd(0);
    let task = ffi_new_task(&prog, &args);
    assert!(!task.is_null());

    let code = unsafe { ffi::aer_task_run(task, None, std::ptr::null_mut()) };
    assert_eq!(code, AerErrorCode::Ok);

    let msg_ptr = ffi::aer_last_error_message();
    assert!(
        msg_ptr.is_null(),
        "error message should be cleared on success"
    );

    unsafe { ffi::aer_task_free(task) };
}

// --- M4b: Observation Tier (Rust API) ---

#[test]
fn capture_output_collects_stdout_chunks() {
    let (prog, args) = noisy_cmd(50);
    let task = Task::new(prog, args).with_capture_output(true);
    let mut events = Vec::new();
    let result = task.run(|e| events.push(e));

    assert!(result.is_ok());
    assert!(matches!(events.first(), Some(Event::Started { .. })));
    assert!(matches!(events.last(), Some(Event::Exited { code: 0, .. })));

    let stdout_chunks: Vec<&Event> = events
        .iter()
        .filter(|e| matches!(e, Event::StdoutChunk { .. }))
        .collect();
    assert!(
        !stdout_chunks.is_empty(),
        "expected at least one StdoutChunk"
    );

    let all_bytes: Vec<u8> = stdout_chunks
        .iter()
        .flat_map(|e| {
            if let Event::StdoutChunk { bytes, .. } = e {
                bytes.clone()
            } else {
                vec![]
            }
        })
        .collect();
    let text = String::from_utf8_lossy(&all_bytes);
    assert!(text.contains('1'), "expected output to contain '1'");
    assert!(text.contains("50"), "expected output to contain '50'");

    // seq monotonically increasing within stdout stream
    let mut prev_seq: Option<u64> = None;
    for e in &stdout_chunks {
        if let Event::StdoutChunk { seq, .. } = e {
            if let Some(prev) = prev_seq {
                assert!(
                    *seq > prev,
                    "seq must be strictly increasing: got {} after {}",
                    seq,
                    prev
                );
            }
            prev_seq = Some(*seq);
        }
    }
}

#[test]
fn capture_output_off_emits_no_chunks() {
    let (prog, args) = noisy_cmd(100);
    let task = Task::new(prog, args); // no with_capture_output
    let mut events = Vec::new();
    let result = task.run(|e| events.push(e));

    assert!(result.is_ok());
    let has_chunks = events
        .iter()
        .any(|e| matches!(e, Event::StdoutChunk { .. } | Event::StderrChunk { .. }));
    assert!(
        !has_chunks,
        "chunks must not be emitted without capture enabled"
    );
    assert_eq!(events.len(), 2, "only Started and Exited without capture");
}

#[test]
fn capture_output_chunks_between_started_and_exited() {
    let (prog, args) = noisy_cmd(10);
    let task = Task::new(prog, args).with_capture_output(true);
    let mut events = Vec::new();
    let result = task.run(|e| events.push(e));

    assert!(result.is_ok());
    let started_pos = events
        .iter()
        .position(|e| matches!(e, Event::Started { .. }))
        .unwrap();
    let exited_pos = events
        .iter()
        .position(|e| matches!(e, Event::Exited { .. }))
        .unwrap();

    for (i, e) in events.iter().enumerate() {
        if matches!(e, Event::StdoutChunk { .. } | Event::StderrChunk { .. }) {
            assert!(
                i > started_pos,
                "chunk at index {} must follow Started at {}",
                i,
                started_pos
            );
            assert!(
                i < exited_pos,
                "chunk at index {} must precede Exited at {}",
                i,
                exited_pos
            );
        }
    }
}

#[test]
fn capture_output_collects_stderr_chunks() {
    let (prog, args) = stderr_cmd("hello_stderr");
    let task = Task::new(prog, args).with_capture_output(true);
    let mut events = Vec::new();
    let result = task.run(|e| events.push(e));

    assert!(result.is_ok());

    let stderr_chunks: Vec<&Event> = events
        .iter()
        .filter(|e| matches!(e, Event::StderrChunk { .. }))
        .collect();
    assert!(
        !stderr_chunks.is_empty(),
        "expected at least one StderrChunk"
    );

    let all_bytes: Vec<u8> = stderr_chunks
        .iter()
        .flat_map(|e| {
            if let Event::StderrChunk { bytes, .. } = e {
                bytes.clone()
            } else {
                vec![]
            }
        })
        .collect();
    let text = String::from_utf8_lossy(&all_bytes);
    assert!(
        text.contains("hello_stderr"),
        "expected 'hello_stderr' in stderr chunks, got: {:?}",
        text
    );
}

// --- M4b: Observation Tier (FFI) ---

/// Accumulates captured output bytes during the FFI callback (pointer is only
/// valid for the duration of the callback, so bytes must be copied immediately).
struct FfiCaptureData {
    started_pid: u32,
    exited_code: i32,
    stdout_chunks: Vec<Vec<u8>>,
    stderr_chunks: Vec<Vec<u8>>,
}

unsafe extern "C" fn collect_ffi_capture(event: *const AerEvent, user_data: *mut c_void) {
    let data = &mut *(user_data as *mut FfiCaptureData);
    let e = &*event;
    match e.kind {
        0 => data.started_pid = e.pid,
        1 => data.exited_code = e.code,
        // Copy bytes immediately — `data` pointer is only valid during the callback
        2 => data
            .stdout_chunks
            .push(std::slice::from_raw_parts(e.data, e.data_len).to_vec()),
        3 => data
            .stderr_chunks
            .push(std::slice::from_raw_parts(e.data, e.data_len).to_vec()),
        _ => {}
    }
}

#[test]
fn ffi_capture_output_collects_stdout() {
    let (prog, args) = noisy_cmd(50);
    let task = ffi_new_task(&prog, &args);
    assert!(!task.is_null());

    let cap_code = unsafe { ffi::aer_task_with_capture_output(task, true) };
    assert_eq!(cap_code, AerErrorCode::Ok);

    let mut data = FfiCaptureData {
        started_pid: 0,
        exited_code: -999,
        stdout_chunks: Vec::new(),
        stderr_chunks: Vec::new(),
    };
    let run_code = unsafe {
        ffi::aer_task_run(
            task,
            Some(collect_ffi_capture),
            &mut data as *mut _ as *mut c_void,
        )
    };
    unsafe { ffi::aer_task_free(task) };

    assert_eq!(run_code, AerErrorCode::Ok);
    assert!(data.started_pid > 0, "started_pid should be > 0");
    assert_eq!(data.exited_code, 0);
    assert!(
        !data.stdout_chunks.is_empty(),
        "expected stdout chunks via FFI"
    );

    let all: Vec<u8> = data.stdout_chunks.into_iter().flatten().collect();
    let text = String::from_utf8_lossy(&all);
    assert!(text.contains('1'), "expected output content in chunks");
}

#[test]
fn ffi_capture_output_off_emits_no_chunks() {
    let (prog, args) = noisy_cmd(50);
    let task = ffi_new_task(&prog, &args);
    assert!(!task.is_null());
    // No aer_task_with_capture_output — capture is off by default

    let mut events: Vec<AerEvent> = Vec::new();
    let run_code = unsafe {
        ffi::aer_task_run(
            task,
            Some(collect_ffi_events),
            &mut events as *mut _ as *mut c_void,
        )
    };
    unsafe { ffi::aer_task_free(task) };

    assert_eq!(run_code, AerErrorCode::Ok);
    let has_chunks = events.iter().any(|e| e.kind == 2 || e.kind == 3);
    assert!(
        !has_chunks,
        "no chunks should be emitted without capture enabled"
    );
    assert_eq!(events.len(), 2, "only Started and Exited without capture");
}

// --- M4c: Cancellation and ExitReason (Rust API) ---

#[test]
fn exit_reason_natural_on_clean_exit() {
    let (prog, args) = exit_cmd(0);
    let task = Task::new(prog, args);
    let mut events = Vec::new();
    let result = task.run(|e| events.push(e));

    assert!(result.is_ok());
    assert!(
        matches!(
            events.last(),
            Some(Event::Exited {
                reason: ExitReason::NaturalExit,
                ..
            })
        ),
        "expected NaturalExit, got {:?}",
        events.last()
    );
}

#[test]
fn exit_reason_timed_out_on_timeout() {
    let (prog, args) = sleep_cmd(60);
    let task = Task::new(prog, args).with_timeout(Duration::from_secs(1));
    let mut events = Vec::new();
    let result = task.run(|e| events.push(e));

    assert!(matches!(result, Err(AerError::TimedOut)));
    assert!(
        matches!(
            events.last(),
            Some(Event::Exited {
                reason: ExitReason::TimedOut,
                ..
            })
        ),
        "expected TimedOut reason, got {:?}",
        events.last()
    );
}

#[test]
fn cancel_kills_running_process() {
    let (prog, args) = sleep_cmd(60);
    let task = Task::new(prog, args);
    let cancel = CancelHandle::new();
    let cancel_clone = cancel.clone();

    let cancel_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        cancel_clone.cancel();
    });

    let mut events = Vec::new();
    let result = task.run_with_cancel(|e| events.push(e), &cancel);
    cancel_thread.join().unwrap();

    assert!(
        matches!(result, Err(AerError::Cancelled)),
        "expected Cancelled, got {:?}",
        result
    );
    assert_eq!(events.len(), 2, "expected Started + Exited");
    assert!(matches!(events[0], Event::Started { .. }));
    assert!(
        matches!(
            events[1],
            Event::Exited {
                code: -1,
                reason: ExitReason::CancelRequested
            }
        ),
        "expected CancelRequested Exited, got {:?}",
        events[1]
    );
}

#[test]
fn cancel_after_exit_is_noop() {
    // cancel() called after the process has already exited must not affect the
    // exit reason — the process should be reported as NaturalExit.
    let (prog, args) = exit_cmd(0);
    let task = Task::new(prog, args);
    let cancel = CancelHandle::new();

    let mut events = Vec::new();
    let result = task.run_with_cancel(|e| events.push(e), &cancel);

    // Process already exited; cancel now is a no-op
    cancel.cancel();

    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert!(
        matches!(
            events.last(),
            Some(Event::Exited {
                reason: ExitReason::NaturalExit,
                ..
            })
        ),
        "cancel after exit must not change reason, got {:?}",
        events.last()
    );
}

#[test]
fn cancel_is_idempotent() {
    let (prog, args) = sleep_cmd(60);
    let task = Task::new(prog, args);
    let cancel = CancelHandle::new();
    let cancel_clone = cancel.clone();

    let cancel_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        cancel_clone.cancel();
        cancel_clone.cancel(); // second call must not panic or error
        cancel_clone.cancel();
    });

    let result = task.run_with_cancel(|_| {}, &cancel);
    cancel_thread.join().unwrap();

    assert!(matches!(result, Err(AerError::Cancelled)));
}

// --- M4c: Cancellation and ExitReason (FFI) ---

#[test]
fn ffi_exit_reason_natural_on_clean_exit() {
    let (prog, args) = exit_cmd(0);
    let task = ffi_new_task(&prog, &args);
    assert!(!task.is_null());

    let mut events: Vec<AerEvent> = Vec::new();
    let code = unsafe {
        ffi::aer_task_run(
            task,
            Some(collect_ffi_events),
            &mut events as *mut _ as *mut c_void,
        )
    };
    unsafe { ffi::aer_task_free(task) };

    assert_eq!(code, AerErrorCode::Ok);
    let exited = events.iter().find(|e| e.kind == 1).unwrap();
    assert_eq!(exited.reason, 0, "expected AER_EXIT_NATURAL (0)");
}

#[test]
fn ffi_exit_reason_timed_out() {
    let (prog, args) = sleep_cmd(60);
    let task = ffi_new_task(&prog, &args);
    assert!(!task.is_null());

    let to_code = unsafe { ffi::aer_task_with_timeout(task, 1000) };
    assert_eq!(to_code, AerErrorCode::Ok);

    let mut events: Vec<AerEvent> = Vec::new();
    let run_code = unsafe {
        ffi::aer_task_run(
            task,
            Some(collect_ffi_events),
            &mut events as *mut _ as *mut c_void,
        )
    };
    unsafe { ffi::aer_task_free(task) };

    assert_eq!(run_code, AerErrorCode::TimedOut);
    let exited = events.iter().find(|e| e.kind == 1).unwrap();
    assert_eq!(exited.reason, 1, "expected AER_EXIT_TIMED_OUT (1)");
    assert_eq!(exited.code, -1);
}

#[test]
fn ffi_cancel_kills_process() {
    let (prog, args) = sleep_cmd(60);
    let task = ffi_new_task(&prog, &args);
    assert!(!task.is_null());

    let cancel = unsafe { ffi::aer_task_make_cancel_handle(task) };
    assert!(!cancel.is_null());

    // Cancel from a separate thread after a brief delay
    let cancel_copy = cancel as usize; // raw pointer as usize for Send
    let cancel_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        let cancel_ptr = cancel_copy as *mut ffi::AerCancelHandle;
        unsafe { ffi::aer_cancel(cancel_ptr) }
    });

    let mut events: Vec<AerEvent> = Vec::new();
    let run_code = unsafe {
        ffi::aer_task_run(
            task,
            Some(collect_ffi_events),
            &mut events as *mut _ as *mut c_void,
        )
    };
    let cancel_result = cancel_thread.join().unwrap();
    unsafe { ffi::aer_cancel_free(cancel) };
    unsafe { ffi::aer_task_free(task) };

    assert_eq!(cancel_result, AerErrorCode::Ok);
    assert_eq!(run_code, AerErrorCode::Cancelled);
    let exited = events.iter().find(|e| e.kind == 1).unwrap();
    assert_eq!(exited.reason, 2, "expected AER_EXIT_CANCEL_REQUESTED (2)");
    assert_eq!(exited.code, -1);
}

#[test]
fn ffi_cancel_null_returns_null_pointer() {
    let code = unsafe { ffi::aer_cancel(std::ptr::null_mut()) };
    assert_eq!(code, AerErrorCode::NullPointer);
}

#[test]
fn ffi_cancel_free_null_is_noop() {
    unsafe { ffi::aer_cancel_free(std::ptr::null_mut()) };
}

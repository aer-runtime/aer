use crate::{AerError, Event, Task};
use std::ffi::{c_char, c_void, CStr, CString};
use std::panic::AssertUnwindSafe;
use std::time::Duration;

// Thread-local stores the last error message so callers can retrieve OS details
// (e.g. ENOENT vs EACCES on spawn failure) without widening the integer ABI.
thread_local! {
    static LAST_ERROR: std::cell::RefCell<Option<CString>> = const { std::cell::RefCell::new(None) };
}

fn set_last_error(msg: String) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = CString::new(msg).ok();
    });
}

fn clear_last_error() {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = None;
    });
}

fn format_panic(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        format!("panic: {}", s)
    } else if let Some(s) = payload.downcast_ref::<String>() {
        format!("panic: {}", s)
    } else {
        "panic: unknown panic payload".to_string()
    }
}

fn map_aer_error(e: AerError) -> AerErrorCode {
    set_last_error(e.to_string());
    match e {
        AerError::SpawnFailed(_) => AerErrorCode::SpawnFailed,
        AerError::WaitFailed(_) => AerErrorCode::WaitFailed,
        AerError::InvalidStateTransition { .. } => AerErrorCode::InvalidStateTransition,
        AerError::TimedOut => AerErrorCode::TimedOut,
        AerError::KillFailed(_) => AerErrorCode::KillFailed,
    }
}

/// Error codes returned by FFI functions. Values are stable ABI.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AerErrorCode {
    Ok = 0,
    NullPointer = 1,
    SpawnFailed = 2,
    WaitFailed = 3,
    InvalidStateTransition = 4,
    TimedOut = 5,
    KillFailed = 6,
    /// Caller invoked aer_task_run a second time on the same handle.
    AlreadyRun = 7,
    /// An unexpected panic was caught at the FFI boundary.
    Panic = 8,
}

/// C-compatible event delivered to AerEventCallback.
///
/// `kind` selects which fields are valid:
///   0 = Started  — `pid` is the process ID
///   1 = Exited   — `code` is the exit code (-1 if killed without a code)
///   2 = StdoutChunk — `seq`, `data`, `data_len` are valid; only when capture enabled
///   3 = StderrChunk — `seq`, `data`, `data_len` are valid; only when capture enabled
///
/// For chunk kinds, `data` points into Rust-owned memory that is only valid for
/// the duration of the callback. Copy the bytes before the callback returns.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AerEvent {
    pub kind: u32,
    pub pid: u32,        // valid when kind == 0
    pub code: i32,       // valid when kind == 1
    pub _pad: u32,       // explicit padding; keeps seq aligned to 8 bytes on all targets
    pub seq: u64,        // valid when kind == 2 or 3; monotonically increasing within stream
    pub data: *const u8, // valid when kind == 2 or 3; only valid during the callback
    pub data_len: usize, // byte count for data
}

// SAFETY: AerEvent is only passed by pointer to C callbacks, never moved across
// threads by Rust. Implementing Send/Sync here is needed only if the struct
// were put in a thread-safe container, which we never do.
// Raw pointer fields make the compiler's auto-impl conservative; the pointer is
// used as a read-only view into a local Vec<u8> valid only during the callback.

/// Nullable C function pointer for receiving events. Pass NULL to ignore events.
pub type AerEventCallback = Option<unsafe extern "C" fn(*const AerEvent, *mut c_void)>;

/// Opaque handle to a Task. Heap-allocated; free with aer_task_free.
pub struct AerTask {
    // Option<Task> so we can call consuming builders:
    // take() → with_timeout() / with_capture_output() → put back.
    inner: Option<Task>,
    has_run: bool,
}

/// Create a new task. Returns NULL on null/invalid-UTF-8 program or any null arg.
///
/// `args` may be NULL when `args_len` is 0.
/// All strings must be valid UTF-8 with no embedded NUL bytes.
/// The returned pointer must be freed with aer_task_free.
///
/// # Safety
/// `program` must be a valid null-terminated C string or NULL.
/// `args` must point to an array of `args_len` valid null-terminated C strings, or NULL when `args_len` is 0.
#[no_mangle]
pub unsafe extern "C" fn aer_task_new(
    program: *const c_char,
    args: *const *const c_char,
    args_len: usize,
) -> *mut AerTask {
    match std::panic::catch_unwind(|| {
        if program.is_null() {
            return std::ptr::null_mut();
        }
        let program_str = match unsafe { CStr::from_ptr(program) }.to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => return std::ptr::null_mut(),
        };

        let mut arg_strings: Vec<String> = Vec::with_capacity(args_len);
        if args_len > 0 {
            if args.is_null() {
                return std::ptr::null_mut();
            }
            for i in 0..args_len {
                let arg_ptr = unsafe { *args.add(i) };
                if arg_ptr.is_null() {
                    return std::ptr::null_mut();
                }
                match unsafe { CStr::from_ptr(arg_ptr) }.to_str() {
                    Ok(s) => arg_strings.push(s.to_owned()),
                    Err(_) => return std::ptr::null_mut(),
                }
            }
        }

        let task = Task::new(program_str, arg_strings);
        Box::into_raw(Box::new(AerTask {
            inner: Some(task),
            has_run: false,
        }))
    }) {
        Ok(ptr) => ptr,
        Err(e) => {
            set_last_error(format_panic(e));
            std::ptr::null_mut()
        }
    }
}

/// Set a timeout on the task. Must be called before aer_task_run.
///
/// # Safety
/// `task` must be a valid pointer returned by aer_task_new that has not been freed.
#[no_mangle]
pub unsafe extern "C" fn aer_task_with_timeout(
    task: *mut AerTask,
    timeout_ms: u64,
) -> AerErrorCode {
    match std::panic::catch_unwind(AssertUnwindSafe(|| {
        if task.is_null() {
            return AerErrorCode::NullPointer;
        }
        let t = unsafe { &mut *task };
        if t.has_run {
            return AerErrorCode::AlreadyRun;
        }
        match t.inner.take() {
            Some(inner) => {
                t.inner = Some(inner.with_timeout(Duration::from_millis(timeout_ms)));
                clear_last_error();
                AerErrorCode::Ok
            }
            None => AerErrorCode::AlreadyRun,
        }
    })) {
        Ok(code) => code,
        Err(e) => {
            set_last_error(format_panic(e));
            AerErrorCode::Panic
        }
    }
}

/// Enable stdout and stderr capture on the task. Must be called before aer_task_run.
///
/// When enabled, StdoutChunk (kind=2) and StderrChunk (kind=3) events are delivered
/// to the callback between Started and Exited. The `data` pointer in those events
/// is only valid for the duration of the callback; copy the bytes if needed later.
///
/// # Safety
/// `task` must be a valid pointer returned by aer_task_new that has not been freed.
#[no_mangle]
pub unsafe extern "C" fn aer_task_with_capture_output(
    task: *mut AerTask,
    capture: bool,
) -> AerErrorCode {
    match std::panic::catch_unwind(AssertUnwindSafe(|| {
        if task.is_null() {
            return AerErrorCode::NullPointer;
        }
        let t = unsafe { &mut *task };
        if t.has_run {
            return AerErrorCode::AlreadyRun;
        }
        match t.inner.take() {
            Some(inner) => {
                t.inner = Some(inner.with_capture_output(capture));
                clear_last_error();
                AerErrorCode::Ok
            }
            None => AerErrorCode::AlreadyRun,
        }
    })) {
        Ok(code) => code,
        Err(e) => {
            set_last_error(format_panic(e));
            AerErrorCode::Panic
        }
    }
}

/// Run the task. May only be called once per handle.
///
/// `callback` may be NULL to ignore events.
/// `user_data` is passed through to the callback unchanged; may be NULL.
/// `user_data` must remain valid for the duration of this call.
///
/// # Safety
/// `task` must be a valid pointer returned by aer_task_new that has not been freed.
/// If `callback` is non-null, it must be a valid function pointer.
/// `user_data` must satisfy any aliasing requirements expected by `callback`.
#[no_mangle]
pub unsafe extern "C" fn aer_task_run(
    task: *mut AerTask,
    callback: AerEventCallback,
    user_data: *mut c_void,
) -> AerErrorCode {
    match std::panic::catch_unwind(AssertUnwindSafe(|| {
        if task.is_null() {
            return AerErrorCode::NullPointer;
        }
        let t = unsafe { &mut *task };
        if t.has_run {
            return AerErrorCode::AlreadyRun;
        }
        t.has_run = true;

        let inner = match t.inner.as_ref() {
            Some(task) => task,
            None => return AerErrorCode::AlreadyRun,
        };

        let result = inner.run(|event| {
            if let Some(cb) = callback {
                match event {
                    Event::Started { pid } => {
                        let c_event = AerEvent {
                            kind: 0,
                            pid,
                            code: 0,
                            _pad: 0,
                            seq: 0,
                            data: std::ptr::null(),
                            data_len: 0,
                        };
                        unsafe { cb(&c_event as *const AerEvent, user_data) };
                    }
                    Event::Exited { code } => {
                        let c_event = AerEvent {
                            kind: 1,
                            pid: 0,
                            code,
                            _pad: 0,
                            seq: 0,
                            data: std::ptr::null(),
                            data_len: 0,
                        };
                        unsafe { cb(&c_event as *const AerEvent, user_data) };
                    }
                    Event::StdoutChunk { seq, bytes } => {
                        let c_event = AerEvent {
                            kind: 2,
                            pid: 0,
                            code: 0,
                            _pad: 0,
                            seq,
                            data: bytes.as_ptr(),
                            data_len: bytes.len(),
                        };
                        // bytes is alive here; pointer valid during cb
                        unsafe { cb(&c_event as *const AerEvent, user_data) };
                    }
                    Event::StderrChunk { seq, bytes } => {
                        let c_event = AerEvent {
                            kind: 3,
                            pid: 0,
                            code: 0,
                            _pad: 0,
                            seq,
                            data: bytes.as_ptr(),
                            data_len: bytes.len(),
                        };
                        unsafe { cb(&c_event as *const AerEvent, user_data) };
                    }
                }
            }
        });

        match result {
            Ok(()) => {
                clear_last_error();
                AerErrorCode::Ok
            }
            Err(e) => map_aer_error(e),
        }
    })) {
        Ok(code) => code,
        Err(e) => {
            set_last_error(format_panic(e));
            AerErrorCode::Panic
        }
    }
}

/// Free a task handle. Safe to call with NULL (no-op).
///
/// # Safety
/// `task` must be a valid pointer returned by aer_task_new, or NULL.
/// Do not use the handle after this call.
#[no_mangle]
pub unsafe extern "C" fn aer_task_free(task: *mut AerTask) {
    if task.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(|| {
        drop(unsafe { Box::from_raw(task) });
    });
}

/// Returns the last error message as a null-terminated C string, or NULL if none.
///
/// The pointer is valid until the next FFI call on this thread. Do not free it.
#[no_mangle]
pub extern "C" fn aer_last_error_message() -> *const c_char {
    LAST_ERROR.with(|e| match &*e.borrow() {
        Some(s) => s.as_ptr(),
        None => std::ptr::null(),
    })
}

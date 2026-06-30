using System.Runtime.InteropServices;

namespace Aer.Core;

/// <summary>
/// Raw P/Invoke declarations matching the stable C ABI in <c>aer.h</c>.
/// All signatures are <c>unsafe</c>-free; higher-level wrappers live in <see cref="AerTask"/>.
/// </summary>
internal static class NativeMethods
{
    private const string Lib = "aer_core";

    /// <summary>
    /// Create a new task. Returns an invalid handle on invalid input.
    /// The returned handle is freed automatically via <see cref="AerTaskHandle.ReleaseHandle"/>.
    /// </summary>
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    public static extern AerTaskHandle aer_task_new(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string program,
        nint args,
        nuint argsLen);

    /// <summary>Set a wall-clock timeout in milliseconds. Must be called before <see cref="aer_task_run"/>.</summary>
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    public static extern AerErrorCode aer_task_with_timeout(nint task, ulong timeoutMs);

    /// <summary>Enable stdout/stderr capture. Must be called before <see cref="aer_task_run"/>.</summary>
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    public static extern AerErrorCode aer_task_with_capture_output(
        nint task,
        [MarshalAs(UnmanagedType.U1)] bool capture);

    /// <summary>
    /// Create a cancellation handle. Must be called before <see cref="aer_task_run"/>.
    /// Returns an invalid handle on failure. Freed automatically via <see cref="AerCancelHandle.ReleaseHandle"/>.
    /// </summary>
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    public static extern AerCancelHandle aer_task_make_cancel_handle(nint task);

    /// <summary>
    /// Spawn the process and block until it exits. <paramref name="callback"/> may be <see langword="null"/>.
    /// A handle may only be run once; a second call returns <see cref="AerErrorCode.AlreadyRun"/>.
    /// </summary>
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    public static extern AerErrorCode aer_task_run(
        nint task,
        AerEventCallback? callback,
        nint userData);

    /// <summary>Free a task handle. Safe to call with <see cref="nint.Zero"/>.</summary>
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    public static extern void aer_task_free(nint task);

    /// <summary>Cancel a running task. Safe to call from any thread at any time.</summary>
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    public static extern AerErrorCode aer_cancel(nint cancel);

    /// <summary>Free a cancel handle. Safe to call with <see cref="nint.Zero"/>.</summary>
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    public static extern void aer_cancel_free(nint cancel);

    /// <summary>
    /// Return the last error message for this thread.
    /// Returns <see cref="nint.Zero"/> if no error since the last successful operation.
    /// Valid until the next FFI call on this thread; do not free.
    /// </summary>
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    public static extern nint aer_last_error_message();
}

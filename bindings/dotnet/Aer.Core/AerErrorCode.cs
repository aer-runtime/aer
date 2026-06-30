namespace Aer.Core;

/// <summary>
/// Return codes from all fallible FFI functions. Integer values are stable ABI — never reorder.
/// </summary>
public enum AerErrorCode : int
{
    Ok = 0,
    NullPointer = 1,
    SpawnFailed = 2,
    WaitFailed = 3,
    InvalidStateTransition = 4,
    TimedOut = 5,
    KillFailed = 6,
    AlreadyRun = 7,
    Panic = 8,
    Cancelled = 9,
}

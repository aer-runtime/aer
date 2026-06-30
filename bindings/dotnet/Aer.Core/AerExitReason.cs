namespace Aer.Core;

/// <summary>
/// Reason an <see cref="AerEvent"/> of kind Exited was delivered. Integer values are stable ABI — never reorder.
/// </summary>
public enum AerExitReason : uint
{
    Natural = 0,
    TimedOut = 1,
    CancelRequested = 2,
}

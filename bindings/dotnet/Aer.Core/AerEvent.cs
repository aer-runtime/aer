using System.Runtime.InteropServices;

namespace Aer.Core;

// Explicit layout must match aer.h exactly (64-bit assumed; all target platforms are 64-bit):
//   offset  0: kind      uint32_t
//   offset  4: pid       uint32_t
//   offset  8: code      int32_t
//   offset 12: reason    uint32_t
//   offset 16: seq       uint64_t
//   offset 24: data      const uint8_t *   (8 bytes on 64-bit)
//   offset 32: data_len  size_t            (8 bytes on 64-bit)
//   total: 40 bytes
[StructLayout(LayoutKind.Explicit, Size = 40)]
public struct AerEvent
{
    [FieldOffset(0)] public uint Kind;
    [FieldOffset(4)] public uint Pid;
    [FieldOffset(8)] public int Code;
    [FieldOffset(12)] public uint Reason;
    [FieldOffset(16)] public ulong Seq;
    [FieldOffset(24)] public nint Data;
    [FieldOffset(32)] public nuint DataLen;
}

public static class AerEventKind
{
    public const uint Started = 0;
    public const uint Exited = 1;
    public const uint StdoutChunk = 2;
    public const uint StderrChunk = 3;
}

// UTF-8 strings for the C ABI.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Runtime.InteropServices;
using System.Text;

namespace Rustorb.Dds;

internal static class Utf8
{
    private static readonly UTF8Encoding Strict = new UTF8Encoding(false, true);

    /// <summary>The NUL-terminated UTF-8 of s.</summary>
    internal static byte[] Z(string s)
    {
        if (s is null)
        {
            throw new ArgumentNullException(nameof(s));
        }
        if (s.IndexOf('\0') >= 0)
        {
            throw new ArgumentException("a string with a NUL character");
        }
        int n = Strict.GetByteCount(s);
        byte[] b = new byte[n + 1];
        Strict.GetBytes(s, 0, s.Length, b, 0);
        return b;
    }

    /// <summary>The UTF-8 of s (no terminator).</summary>
    internal static byte[] Bytes(string s) => Strict.GetBytes(s);

    internal static unsafe string Read(byte* p, nuint len) =>
        len == 0 ? "" : Strict.GetString(p, checked((int)len));

    internal static string? FromZ(nint p) => p == 0 ? null : Marshal.PtrToStringUTF8(p);
}

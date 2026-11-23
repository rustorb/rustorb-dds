// ReturnCode_t to exceptions, with the message of rdds_last_error_message.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Runtime.InteropServices;
using Omg.Dds.Core;

namespace Rustorb.Dds.Native;

internal static class Status
{
    internal const int OK = 0;
    internal const int ERROR = 1;
    internal const int UNSUPPORTED = 2;
    internal const int BAD_PARAMETER = 3;
    internal const int PRECONDITION_NOT_MET = 4;
    internal const int OUT_OF_RESOURCES = 5;
    internal const int NOT_ENABLED = 6;
    internal const int IMMUTABLE_POLICY = 7;
    internal const int INCONSISTENT_POLICY = 8;
    internal const int ALREADY_DELETED = 9;
    internal const int TIMEOUT = 10;
    internal const int NO_DATA = 11;
    internal const int ILLEGAL_OPERATION = 12;
}

internal static class Errors
{
    /// <summary>Throws the exception of a failed return code.</summary>
    internal static void Check(int status)
    {
        if (status != Status.OK)
        {
            throw Of(status);
        }
    }

    internal static unsafe string LastMessage()
    {
        nint p = (nint)Ffi.rdds_last_error_message();
        string? m = p == 0 ? null : Marshal.PtrToStringUTF8(p);
        return string.IsNullOrEmpty(m) ? "rustorb-dds failed" : m!;
    }

    internal static Exception Of(int status)
    {
        string m = LastMessage();
        return status switch
        {
            Status.UNSUPPORTED => new NotSupportedException(m),
            Status.BAD_PARAMETER => new ArgumentException(m),
            Status.PRECONDITION_NOT_MET => new PreconditionNotMetException(m),
            Status.OUT_OF_RESOURCES => new OutOfResourcesException(m),
            Status.NOT_ENABLED => new NotEnabledException(m),
            Status.IMMUTABLE_POLICY => new ImmutablePolicyException(m),
            Status.INCONSISTENT_POLICY => new InconsistentPolicyException(m),
            Status.ALREADY_DELETED => new AlreadyClosedException(m),
            Status.TIMEOUT => new TimeoutException(m),
            Status.ILLEGAL_OPERATION => new IllegalOperationException(m),
            _ => new DdsException(m),
        };
    }
}

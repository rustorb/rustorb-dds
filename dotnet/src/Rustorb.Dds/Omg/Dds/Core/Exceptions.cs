// The exceptions of the DDS return codes (DDS 1.4, 2.2.1.1), after table
// 7.1 of the Java 5 PSM: BAD_PARAMETER is an ArgumentException,
// UNSUPPORTED a NotSupportedException and TIMEOUT a TimeoutException;
// the others derive from DdsException.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;

namespace Omg.Dds.Core;

/// <summary>RETCODE_ERROR, and the base of the DDS exceptions.</summary>
public class DdsException : Exception
{
    public DdsException(string message) : base(message) { }
}

/// <summary>RETCODE_ALREADY_DELETED, or an entity used after Dispose.</summary>
public class AlreadyClosedException : DdsException
{
    public AlreadyClosedException(string message) : base(message) { }
}

/// <summary>RETCODE_ILLEGAL_OPERATION.</summary>
public class IllegalOperationException : DdsException
{
    public IllegalOperationException(string message) : base(message) { }
}

/// <summary>RETCODE_IMMUTABLE_POLICY.</summary>
public class ImmutablePolicyException : DdsException
{
    public ImmutablePolicyException(string message) : base(message) { }
}

/// <summary>RETCODE_INCONSISTENT_POLICY.</summary>
public class InconsistentPolicyException : DdsException
{
    public InconsistentPolicyException(string message) : base(message) { }
}

/// <summary>RETCODE_NOT_ENABLED.</summary>
public class NotEnabledException : DdsException
{
    public NotEnabledException(string message) : base(message) { }
}

/// <summary>RETCODE_OUT_OF_RESOURCES.</summary>
public class OutOfResourcesException : DdsException
{
    public OutOfResourcesException(string message) : base(message) { }
}

/// <summary>RETCODE_PRECONDITION_NOT_MET.</summary>
public class PreconditionNotMetException : DdsException
{
    public PreconditionNotMetException(string message) : base(message) { }
}

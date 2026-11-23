// SafeHandles for the handles of the C ABI: each is released once, by its
// rdds_*_release function.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System.Runtime.InteropServices;

namespace Rustorb.Dds.Native;

internal abstract class Handle : SafeHandle
{
    protected Handle(nint h) : base(0, true)
    {
        SetHandle(h);
    }

    public override bool IsInvalid => handle == 0;

    /// <summary>The raw handle; the caller keeps this object alive
    /// (GC.KeepAlive) while it uses it. Throws once it is closed.</summary>
    internal nint Raw
    {
        get
        {
            if (IsClosed)
            {
                throw new Omg.Dds.Core.AlreadyClosedException("the entity is closed");
            }
            return handle;
        }
    }
}

internal sealed class TypeHandle : Handle
{
    internal TypeHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_type_release(handle); return true; }
}

internal sealed class OutHandle : Handle
{
    internal OutHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_out_release(handle); return true; }
}

internal sealed class InHandle : Handle
{
    internal InHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_in_release(handle); return true; }
}

internal sealed class ParticipantHandle : Handle
{
    internal ParticipantHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_participant_release(handle); return true; }
}

internal sealed class TopicHandle : Handle
{
    internal TopicHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_topic_release(handle); return true; }
}

internal sealed class CfTopicHandle : Handle
{
    internal CfTopicHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_cftopic_release(handle); return true; }
}

internal sealed class PublisherHandle : Handle
{
    internal PublisherHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_publisher_release(handle); return true; }
}

internal sealed class SubscriberHandle : Handle
{
    internal SubscriberHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_subscriber_release(handle); return true; }
}

internal sealed class WriterHandle : Handle
{
    internal WriterHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_writer_release(handle); return true; }
}

internal sealed class ReaderHandle : Handle
{
    internal ReaderHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_reader_release(handle); return true; }
}

internal sealed class SamplesHandle : Handle
{
    internal SamplesHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_samples_release(handle); return true; }
}

internal sealed class ConditionHandle : Handle
{
    internal ConditionHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_condition_release(handle); return true; }
}

internal sealed class WaitSetHandle : Handle
{
    internal WaitSetHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_waitset_release(handle); return true; }
}

internal sealed class RequesterHandle : Handle
{
    internal RequesterHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_requester_release(handle); return true; }
}

internal sealed class ReplierHandle : Handle
{
    internal ReplierHandle(nint h) : base(h) { }
    protected override bool ReleaseHandle() { Ffi.rdds_replier_release(handle); return true; }
}

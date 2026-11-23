// The listener tables of the C ABI (rdds_writer_listener, rdds_reader_listener,
// rdds_listener): static [UnmanagedCallersOnly] trampolines whose self is a
// GCHandle of a state object. The library calls release once when the
// listener is replaced or the entity deleted; that frees the GCHandle.
// A call that comes before the C# entity is made waits in the state until
// it is. Exceptions of a listener are not let into the library.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Threading;
using Omg.Dds.Core;
using Omg.Dds.Core.Status;

namespace Rustorb.Dds.Native;

/// <summary>The state behind a listener table's self.</summary>
internal abstract class ListenerState
{
    private object? entity;
    private List<Action<object>>? pending = new();
    private GCHandle self;
    private int freed;

    internal nint Self
    {
        get
        {
            if (!self.IsAllocated)
            {
                self = GCHandle.Alloc(this);
            }
            return GCHandle.ToIntPtr(self);
        }
    }

    /// <summary>The C# entity is made: delivers what waited for it.</summary>
    internal void Attach(object e)
    {
        List<Action<object>>? waiting;
        lock (this)
        {
            entity = e;
            waiting = pending;
            pending = null;
        }
        if (waiting != null)
        {
            foreach (Action<object> a in waiting)
            {
                Run(a, e);
            }
        }
    }

    internal void Deliver(Action<object> a)
    {
        object? e;
        lock (this)
        {
            e = entity;
            if (e is null)
            {
                pending?.Add(a);
                return;
            }
        }
        Run(a, e);
    }

    private static void Run(Action<object> a, object e)
    {
        try
        {
            a(e);
        }
        catch (Exception ex)
        {
            // A listener's exception must not unwind into the library.
            System.Diagnostics.Trace.TraceError("rustorb-dds: a listener threw {0}", ex);
        }
    }

    /// <summary>Frees the GCHandle, once (the library's release, or a
    /// failed creation).</summary>
    internal void Free()
    {
        if (Interlocked.Exchange(ref freed, 1) == 0 && self.IsAllocated)
        {
            self.Free();
        }
    }

    internal static ListenerState Of(nint self) => (ListenerState)GCHandle.FromIntPtr(self).Target!;
}

internal abstract class WriterListenerState : ListenerState
{
    internal abstract void DeadlineMissed(OfferedDeadlineMissedStatus s);
    internal abstract void IncompatibleQos(OfferedIncompatibleQosStatus s);
    internal abstract void LivelinessLost(LivelinessLostStatus s);
    internal abstract void PublicationMatched(PublicationMatchedStatus s);
}

internal abstract class ReaderListenerState : ListenerState
{
    internal abstract void DeadlineMissed(RequestedDeadlineMissedStatus s);
    internal abstract void IncompatibleQos(RequestedIncompatibleQosStatus s);
    internal abstract void SampleRejected(SampleRejectedStatus s);
    internal abstract void LivelinessChanged(LivelinessChangedStatus s);
    internal abstract void DataAvailable();
    internal abstract void SubscriptionMatched(SubscriptionMatchedStatus s);
    internal abstract void SampleLost(SampleLostStatus s);
}

/// <summary>The state of an rdds_listener (participant, publisher,
/// subscriber or topic): the endpoint of a status is found by its GUID.</summary>
internal abstract class GroupListenerState : ListenerState
{
    internal abstract void WriterStatus(InstanceHandle guid, Action<object, Omg.Dds.Pub.IPublisherListener> call);
    internal abstract void ReaderStatus(InstanceHandle guid, Action<object, Omg.Dds.Sub.ISubscriberListener> call);
    internal abstract void DataOnReaders(nint subscriber);
    internal abstract void InconsistentTopic(string name, InconsistentTopicStatus s);
}

internal static unsafe class ListenerTables
{
    internal static RddsWriterListener Writer(WriterListenerState state) => new()
    {
        self = state.Self,
        on_offered_deadline_missed = (nint)(delegate* unmanaged[Cdecl]<nint, RddsDeadlineMissedStatus*, void>)&WDeadline,
        on_offered_incompatible_qos = (nint)(delegate* unmanaged[Cdecl]<nint, RddsIncompatibleQosStatus*, void>)&WIncompatible,
        on_liveliness_lost = (nint)(delegate* unmanaged[Cdecl]<nint, RddsCountStatus*, void>)&WLiveliness,
        on_publication_matched = (nint)(delegate* unmanaged[Cdecl]<nint, RddsMatchedStatus*, void>)&WMatched,
        release = (nint)(delegate* unmanaged[Cdecl]<nint, void>)&Release,
    };

    internal static RddsReaderListener Reader(ReaderListenerState state) => new()
    {
        self = state.Self,
        on_requested_deadline_missed = (nint)(delegate* unmanaged[Cdecl]<nint, RddsDeadlineMissedStatus*, void>)&RDeadline,
        on_requested_incompatible_qos = (nint)(delegate* unmanaged[Cdecl]<nint, RddsIncompatibleQosStatus*, void>)&RIncompatible,
        on_sample_rejected = (nint)(delegate* unmanaged[Cdecl]<nint, RddsSampleRejectedStatus*, void>)&RRejected,
        on_liveliness_changed = (nint)(delegate* unmanaged[Cdecl]<nint, RddsLivelinessChangedStatus*, void>)&RLiveliness,
        on_data_available = (nint)(delegate* unmanaged[Cdecl]<nint, void>)&RData,
        on_subscription_matched = (nint)(delegate* unmanaged[Cdecl]<nint, RddsMatchedStatus*, void>)&RMatched,
        on_sample_lost = (nint)(delegate* unmanaged[Cdecl]<nint, RddsCountStatus*, void>)&RLost,
        release = (nint)(delegate* unmanaged[Cdecl]<nint, void>)&Release,
    };

    internal static RddsListener Group(GroupListenerState state) => new()
    {
        self = state.Self,
        on_offered_deadline_missed = (nint)(delegate* unmanaged[Cdecl]<nint, RddsEndpointInfo*, RddsDeadlineMissedStatus*, void>)&GWDeadline,
        on_offered_incompatible_qos = (nint)(delegate* unmanaged[Cdecl]<nint, RddsEndpointInfo*, RddsIncompatibleQosStatus*, void>)&GWIncompatible,
        on_liveliness_lost = (nint)(delegate* unmanaged[Cdecl]<nint, RddsEndpointInfo*, RddsCountStatus*, void>)&GWLiveliness,
        on_publication_matched = (nint)(delegate* unmanaged[Cdecl]<nint, RddsEndpointInfo*, RddsMatchedStatus*, void>)&GWMatched,
        on_data_on_readers = (nint)(delegate* unmanaged[Cdecl]<nint, nint, void>)&GDataOnReaders,
        on_requested_deadline_missed = (nint)(delegate* unmanaged[Cdecl]<nint, RddsEndpointInfo*, RddsDeadlineMissedStatus*, void>)&GRDeadline,
        on_requested_incompatible_qos = (nint)(delegate* unmanaged[Cdecl]<nint, RddsEndpointInfo*, RddsIncompatibleQosStatus*, void>)&GRIncompatible,
        on_sample_rejected = (nint)(delegate* unmanaged[Cdecl]<nint, RddsEndpointInfo*, RddsSampleRejectedStatus*, void>)&GRRejected,
        on_liveliness_changed = (nint)(delegate* unmanaged[Cdecl]<nint, RddsEndpointInfo*, RddsLivelinessChangedStatus*, void>)&GRLiveliness,
        on_data_available = (nint)(delegate* unmanaged[Cdecl]<nint, RddsEndpointInfo*, void>)&GRData,
        on_subscription_matched = (nint)(delegate* unmanaged[Cdecl]<nint, RddsEndpointInfo*, RddsMatchedStatus*, void>)&GRMatched,
        on_sample_lost = (nint)(delegate* unmanaged[Cdecl]<nint, RddsEndpointInfo*, RddsCountStatus*, void>)&GRLost,
        on_inconsistent_topic = (nint)(delegate* unmanaged[Cdecl]<nint, byte*, RddsCountStatus*, void>)&GTopic,
        release = (nint)(delegate* unmanaged[Cdecl]<nint, void>)&Release,
    };

    private static void Guard(Action a)
    {
        try
        {
            a();
        }
        catch (Exception ex)
        {
            System.Diagnostics.Trace.TraceError("rustorb-dds: a listener threw {0}", ex);
        }
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void Release(nint self) => Guard(() => ListenerState.Of(self).Free());

    // rdds_writer_listener

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void WDeadline(nint self, RddsDeadlineMissedStatus* s)
    {
        var v = OfferedDeadlineMissedStatus.Of(*s);
        Guard(() => ((WriterListenerState)ListenerState.Of(self)).DeadlineMissed(v));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void WIncompatible(nint self, RddsIncompatibleQosStatus* s)
    {
        var v = OfferedIncompatibleQosStatus.Of(*s);
        Guard(() => ((WriterListenerState)ListenerState.Of(self)).IncompatibleQos(v));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void WLiveliness(nint self, RddsCountStatus* s)
    {
        var v = LivelinessLostStatus.Of(*s);
        Guard(() => ((WriterListenerState)ListenerState.Of(self)).LivelinessLost(v));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void WMatched(nint self, RddsMatchedStatus* s)
    {
        var v = PublicationMatchedStatus.Of(*s);
        Guard(() => ((WriterListenerState)ListenerState.Of(self)).PublicationMatched(v));
    }

    // rdds_reader_listener

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void RDeadline(nint self, RddsDeadlineMissedStatus* s)
    {
        var v = RequestedDeadlineMissedStatus.Of(*s);
        Guard(() => ((ReaderListenerState)ListenerState.Of(self)).DeadlineMissed(v));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void RIncompatible(nint self, RddsIncompatibleQosStatus* s)
    {
        var v = RequestedIncompatibleQosStatus.Of(*s);
        Guard(() => ((ReaderListenerState)ListenerState.Of(self)).IncompatibleQos(v));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void RRejected(nint self, RddsSampleRejectedStatus* s)
    {
        var v = SampleRejectedStatus.Of(*s);
        Guard(() => ((ReaderListenerState)ListenerState.Of(self)).SampleRejected(v));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void RLiveliness(nint self, RddsLivelinessChangedStatus* s)
    {
        var v = LivelinessChangedStatus.Of(*s);
        Guard(() => ((ReaderListenerState)ListenerState.Of(self)).LivelinessChanged(v));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void RData(nint self) =>
        Guard(() => ((ReaderListenerState)ListenerState.Of(self)).DataAvailable());

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void RMatched(nint self, RddsMatchedStatus* s)
    {
        var v = SubscriptionMatchedStatus.Of(*s);
        Guard(() => ((ReaderListenerState)ListenerState.Of(self)).SubscriptionMatched(v));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void RLost(nint self, RddsCountStatus* s)
    {
        var v = SampleLostStatus.Of(*s);
        Guard(() => ((ReaderListenerState)ListenerState.Of(self)).SampleLost(v));
    }

    // rdds_listener

    private static GroupListenerState G(nint self) => (GroupListenerState)ListenerState.Of(self);

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void GWDeadline(nint self, RddsEndpointInfo* w, RddsDeadlineMissedStatus* s)
    {
        var g = InstanceHandle.Of(w->guid);
        var v = OfferedDeadlineMissedStatus.Of(*s);
        Guard(() => G(self).WriterStatus(g, (e, l) => l.OnOfferedDeadlineMissed((Omg.Dds.Pub.IDataWriter)e, v)));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void GWIncompatible(nint self, RddsEndpointInfo* w, RddsIncompatibleQosStatus* s)
    {
        var g = InstanceHandle.Of(w->guid);
        var v = OfferedIncompatibleQosStatus.Of(*s);
        Guard(() => G(self).WriterStatus(g, (e, l) => l.OnOfferedIncompatibleQos((Omg.Dds.Pub.IDataWriter)e, v)));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void GWLiveliness(nint self, RddsEndpointInfo* w, RddsCountStatus* s)
    {
        var g = InstanceHandle.Of(w->guid);
        var v = LivelinessLostStatus.Of(*s);
        Guard(() => G(self).WriterStatus(g, (e, l) => l.OnLivelinessLost((Omg.Dds.Pub.IDataWriter)e, v)));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void GWMatched(nint self, RddsEndpointInfo* w, RddsMatchedStatus* s)
    {
        var g = InstanceHandle.Of(w->guid);
        var v = PublicationMatchedStatus.Of(*s);
        Guard(() => G(self).WriterStatus(g, (e, l) => l.OnPublicationMatched((Omg.Dds.Pub.IDataWriter)e, v)));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void GDataOnReaders(nint self, nint subscriber) =>
        Guard(() => G(self).DataOnReaders(subscriber));

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void GRDeadline(nint self, RddsEndpointInfo* r, RddsDeadlineMissedStatus* s)
    {
        var g = InstanceHandle.Of(r->guid);
        var v = RequestedDeadlineMissedStatus.Of(*s);
        Guard(() => G(self).ReaderStatus(g, (e, l) => l.OnRequestedDeadlineMissed((Omg.Dds.Sub.IDataReader)e, v)));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void GRIncompatible(nint self, RddsEndpointInfo* r, RddsIncompatibleQosStatus* s)
    {
        var g = InstanceHandle.Of(r->guid);
        var v = RequestedIncompatibleQosStatus.Of(*s);
        Guard(() => G(self).ReaderStatus(g, (e, l) => l.OnRequestedIncompatibleQos((Omg.Dds.Sub.IDataReader)e, v)));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void GRRejected(nint self, RddsEndpointInfo* r, RddsSampleRejectedStatus* s)
    {
        var g = InstanceHandle.Of(r->guid);
        var v = SampleRejectedStatus.Of(*s);
        Guard(() => G(self).ReaderStatus(g, (e, l) => l.OnSampleRejected((Omg.Dds.Sub.IDataReader)e, v)));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void GRLiveliness(nint self, RddsEndpointInfo* r, RddsLivelinessChangedStatus* s)
    {
        var g = InstanceHandle.Of(r->guid);
        var v = LivelinessChangedStatus.Of(*s);
        Guard(() => G(self).ReaderStatus(g, (e, l) => l.OnLivelinessChanged((Omg.Dds.Sub.IDataReader)e, v)));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void GRData(nint self, RddsEndpointInfo* r)
    {
        var g = InstanceHandle.Of(r->guid);
        Guard(() => G(self).ReaderStatus(g, (e, l) => l.OnDataAvailable((Omg.Dds.Sub.IDataReader)e)));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void GRMatched(nint self, RddsEndpointInfo* r, RddsMatchedStatus* s)
    {
        var g = InstanceHandle.Of(r->guid);
        var v = SubscriptionMatchedStatus.Of(*s);
        Guard(() => G(self).ReaderStatus(g, (e, l) => l.OnSubscriptionMatched((Omg.Dds.Sub.IDataReader)e, v)));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void GRLost(nint self, RddsEndpointInfo* r, RddsCountStatus* s)
    {
        var g = InstanceHandle.Of(r->guid);
        var v = SampleLostStatus.Of(*s);
        Guard(() => G(self).ReaderStatus(g, (e, l) => l.OnSampleLost((Omg.Dds.Sub.IDataReader)e, v)));
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void GTopic(nint self, byte* name, RddsCountStatus* s)
    {
        string n = Utf8.FromZ((nint)name) ?? "";
        var v = InconsistentTopicStatus.Of(*s);
        Guard(() => G(self).InconsistentTopic(n, v));
    }
}

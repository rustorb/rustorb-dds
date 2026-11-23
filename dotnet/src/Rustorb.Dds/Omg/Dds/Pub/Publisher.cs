// Publisher and DataWriter (DDS 1.4, 2.2.2.4).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections.Generic;
using Omg.Dds.Core;
using Omg.Dds.Core.Status;
using Omg.Dds.Domain;
using Omg.Dds.Topic;
using Rustorb.Dds;
using Rustorb.Dds.Native;

namespace Omg.Dds.Pub;

public sealed unsafe class Publisher : Entity
{
    internal readonly PublisherHandle handle;
    private readonly List<Entity> writers = new();

    internal Publisher(DomainParticipant participant, PublisherHandle handle, PublisherQos qos)
    {
        Participant = participant;
        this.handle = handle;
        Qos = qos;
    }

    internal nint Raw => handle.Raw;

    public DomainParticipant Participant { get; }

    public PublisherQos Qos { get; }

    public override InstanceHandle InstanceHandle => InstanceHandle.Nil;

    /// <summary>rustorb-dds's default writer QoS; change it with `with`.</summary>
    public DataWriterQos DefaultDataWriterQos => DataWriterQos.Default;

    public DataWriter<T> CreateDataWriter<T>(Topic<T> topic, DataWriterQos? qos = null,
        IDataWriterListener<T>? listener = null, StatusKind mask = StatusKind.All)
    {
        qos ??= DataWriterQos.Default;
        WriterState<T>? state = listener is null ? null : new WriterState<T>(listener);
        nint w;
        try
        {
            w = qos.With(q =>
            {
                nint h;
                if (state is null)
                {
                    Errors.Check(Ffi.rdds_publisher_create_writer(Raw, topic.Raw, q, null, (uint)mask, &h));
                }
                else
                {
                    RddsWriterListener table = ListenerTables.Writer(state);
                    Errors.Check(Ffi.rdds_publisher_create_writer(Raw, topic.Raw, q, &table, (uint)mask, &h));
                }
                return h;
            });
        }
        catch
        {
            state?.Free();
            throw;
        }
        GC.KeepAlive(this);
        GC.KeepAlive(topic);
        var writer = new DataWriter<T>(this, topic, new WriterHandle(w), qos);
        state?.Attach(writer);
        lock (writers)
        {
            writers.Add(writer);
        }
        return writer;
    }

    /// <summary>Sets the listener (null clears it).</summary>
    public void SetListener(IPublisherListener? listener, StatusKind mask = StatusKind.All)
    {
        Participant.SetGroupListener(listener, mask, (l, m) => Ffi.rdds_publisher_set_listener(Raw, (RddsListener*)l, m));
        GC.KeepAlive(this);
    }

    internal void Remove(Entity w)
    {
        lock (writers)
        {
            writers.Remove(w);
        }
    }

    public void DeleteContainedEntities()
    {
        List<Entity> all;
        lock (writers)
        {
            all = new List<Entity>(writers);
            writers.Clear();
        }
        foreach (Entity w in all)
        {
            w.Dispose();
        }
    }

    protected override void Close()
    {
        DeleteContainedEntities();
        Participant.Remove(this);
        handle.Dispose();
    }

    private sealed class WriterState<T> : WriterListenerState
    {
        private readonly IDataWriterListener<T> l;

        internal WriterState(IDataWriterListener<T> l) => this.l = l;

        internal override void DeadlineMissed(OfferedDeadlineMissedStatus s) =>
            Deliver(w => l.OnOfferedDeadlineMissed((DataWriter<T>)w, s));

        internal override void IncompatibleQos(OfferedIncompatibleQosStatus s) =>
            Deliver(w => l.OnOfferedIncompatibleQos((DataWriter<T>)w, s));

        internal override void LivelinessLost(LivelinessLostStatus s) =>
            Deliver(w => l.OnLivelinessLost((DataWriter<T>)w, s));

        internal override void PublicationMatched(PublicationMatchedStatus s) =>
            Deliver(w => l.OnPublicationMatched((DataWriter<T>)w, s));
    }

    internal static WriterListenerState State<T>(IDataWriterListener<T> l) => new WriterState<T>(l);
}

/// <summary>A writer, whatever its type (for the listeners of publishers
/// and participants).</summary>
public interface IDataWriter : IDisposable
{
    InstanceHandle InstanceHandle { get; }
    ITopic Topic { get; }
    Publisher Publisher { get; }
    void WaitForAcknowledgments(Duration maxWait);
    /// <summary>Waits until the writer matches count readers (rustorb-dds).</summary>
    void WaitForSubscriptions(int count, Duration timeout);
}

/// <summary>A writer of T.</summary>
public sealed unsafe class DataWriter<T> : Entity, IDataWriter
{
    internal readonly WriterHandle handle;
    private readonly ICodec<T> codec;
    private readonly InstanceHandle guid;
    private readonly object sync = new();

    internal DataWriter(Publisher publisher, Topic<T> topic, WriterHandle handle, DataWriterQos qos)
    {
        Publisher = publisher;
        Topic = topic;
        this.handle = handle;
        codec = topic.codec;
        Qos = qos;
        RddsGuid g;
        Errors.Check(Ffi.rdds_writer_guid(handle.Raw, &g));
        guid = InstanceHandle.Of(g);
        Endpoints.Add(guid, this);
    }

    internal nint Raw => handle.Raw;

    public Publisher Publisher { get; }

    public Topic<T> Topic { get; }

    ITopic IDataWriter.Topic => Topic;

    public DataWriterQos Qos { get; }

    /// <summary>The writer's GUID.</summary>
    public override InstanceHandle InstanceHandle => guid;

    /// <summary>A stream with the sample (a new one each time: a listener
    /// may write from inside a write).</summary>
    private Out Fill(T sample)
    {
        if (sample is null)
        {
            throw new ArgumentNullException(nameof(sample));
        }
        var o = new Out();
        try
        {
            codec.Write(o, sample);
        }
        catch
        {
            o.Dispose();
            throw;
        }
        return o;
    }

    public void Write(T sample) => Write(sample, Time.Invalid);

    /// <summary>Writes a sample with its source timestamp.</summary>
    public void Write(T sample, Time timestamp)
    {
        RddsTime t = timestamp.Native;
        int rc;
        using (Out o = Fill(sample))
        {
            rc = Ffi.rdds_writer_write(Raw, o.Raw, timestamp.IsValid ? &t : null);
        }
        GC.KeepAlive(this);
        Errors.Check(rc);
    }

    public InstanceHandle RegisterInstance(T instance)
    {
        RddsInstanceHandle h = default;
        int rc;
        using (Out o = Fill(instance))
        {
            rc = Ffi.rdds_writer_register_instance(Raw, o.Raw, &h);
        }
        GC.KeepAlive(this);
        Errors.Check(rc);
        return InstanceHandle.Of(h);
    }

    public void UnregisterInstance(T instance) => UnregisterInstance(instance, Time.Invalid);

    public void UnregisterInstance(T instance, Time timestamp)
    {
        RddsTime t = timestamp.Native;
        int rc;
        using (Out o = Fill(instance))
        {
            rc = Ffi.rdds_writer_unregister_instance(Raw, o.Raw, timestamp.IsValid ? &t : null);
        }
        GC.KeepAlive(this);
        Errors.Check(rc);
    }

    /// <summary>Disposes the instance of the sample's key (DataWriter::dispose;
    /// Dispose() deletes the writer).</summary>
    public void DisposeInstance(T instance) => DisposeInstance(instance, Time.Invalid);

    public void DisposeInstance(T instance, Time timestamp)
    {
        RddsTime t = timestamp.Native;
        int rc;
        using (Out o = Fill(instance))
        {
            rc = Ffi.rdds_writer_dispose(Raw, o.Raw, timestamp.IsValid ? &t : null);
        }
        GC.KeepAlive(this);
        Errors.Check(rc);
    }

    /// <summary>The handle of the sample's instance; Nil when the writer does
    /// not know it.</summary>
    public InstanceHandle LookupInstance(T instance)
    {
        RddsInstanceHandle h = default;
        int rc;
        using (Out o = Fill(instance))
        {
            rc = Ffi.rdds_writer_lookup_instance(Raw, o.Raw, &h);
        }
        GC.KeepAlive(this);
        Errors.Check(rc);
        return InstanceHandle.Of(h);
    }

    /// <summary>Until the reliable readers acknowledged every sample;
    /// TimeoutException.</summary>
    public void WaitForAcknowledgments(Duration maxWait)
    {
        Errors.Check(Ffi.rdds_writer_wait_for_acknowledgments(Raw, maxWait.Native));
        GC.KeepAlive(this);
    }

    /// <summary>Until count readers match; TimeoutException (rustorb's extension).</summary>
    public void WaitForSubscriptions(int count, Duration timeout)
    {
        Errors.Check(Ffi.rdds_writer_wait_for_subscriptions(Raw, (nuint)count, timeout.Native));
        GC.KeepAlive(this);
    }

    /// <summary>The GUIDs of the matched readers.</summary>
    public IReadOnlyList<InstanceHandle> GetMatchedSubscriptions()
    {
        nuint n;
        Errors.Check(Ffi.rdds_writer_matched_subscriptions(Raw, null, 0, &n));
        var buf = new RddsGuid[(int)n + 8];
        fixed (RddsGuid* p = buf)
        {
            Errors.Check(Ffi.rdds_writer_matched_subscriptions(Raw, p, (nuint)buf.Length, &n));
        }
        GC.KeepAlive(this);
        var r = new List<InstanceHandle>();
        for (int i = 0; i < (int)Math.Min(n, (nuint)buf.Length); i++)
        {
            r.Add(InstanceHandle.Of(buf[i]));
        }
        return r;
    }

    public void AssertLiveliness()
    {
        Errors.Check(Ffi.rdds_writer_assert_liveliness(Raw));
        GC.KeepAlive(this);
    }

    public PublicationMatchedStatus GetPublicationMatchedStatus()
    {
        RddsMatchedStatus s;
        Errors.Check(Ffi.rdds_writer_publication_matched_status(Raw, &s));
        GC.KeepAlive(this);
        return PublicationMatchedStatus.Of(s);
    }

    public OfferedIncompatibleQosStatus GetOfferedIncompatibleQosStatus()
    {
        RddsIncompatibleQosStatus s;
        Errors.Check(Ffi.rdds_writer_offered_incompatible_qos_status(Raw, &s));
        GC.KeepAlive(this);
        return OfferedIncompatibleQosStatus.Of(s);
    }

    public LivelinessLostStatus GetLivelinessLostStatus()
    {
        RddsCountStatus s;
        Errors.Check(Ffi.rdds_writer_liveliness_lost_status(Raw, &s));
        GC.KeepAlive(this);
        return LivelinessLostStatus.Of(s);
    }

    public OfferedDeadlineMissedStatus GetOfferedDeadlineMissedStatus()
    {
        RddsDeadlineMissedStatus s;
        Errors.Check(Ffi.rdds_writer_offered_deadline_missed_status(Raw, &s));
        GC.KeepAlive(this);
        return OfferedDeadlineMissedStatus.Of(s);
    }

    /// <summary>The statuses that changed since they were last read.</summary>
    public StatusKind StatusChanges
    {
        get
        {
            uint m = Ffi.rdds_writer_status_changes(Raw);
            GC.KeepAlive(this);
            return (StatusKind)m;
        }
    }

    private StatusCondition? statusCondition;

    public StatusCondition StatusCondition
    {
        get
        {
            lock (sync)
            {
                if (statusCondition is null)
                {
                    nint c;
                    Errors.Check(Ffi.rdds_writer_status_condition(Raw, &c));
                    GC.KeepAlive(this);
                    statusCondition = new StatusCondition(new ConditionHandle(c), this);
                }
                return statusCondition;
            }
        }
    }

    /// <summary>Sets the listener (null clears it).</summary>
    public void SetListener(IDataWriterListener<T>? listener, StatusKind mask = StatusKind.All)
    {
        if (listener is null)
        {
            Errors.Check(Ffi.rdds_writer_set_listener(Raw, null, (uint)mask));
            GC.KeepAlive(this);
            return;
        }
        WriterListenerState state = Publisher.State(listener);
        state.Attach(this);
        RddsWriterListener table = ListenerTables.Writer(state);
        int rc = Ffi.rdds_writer_set_listener(Raw, &table, (uint)mask);
        GC.KeepAlive(this);
        if (rc != Status.OK)
        {
            state.Free();
            Errors.Check(rc);
        }
    }

    protected override void Close()
    {
        Endpoints.Remove(guid);
        Publisher.Remove(this);
        handle.Dispose();
    }
}

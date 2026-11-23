// Subscriber, DataReader and samples (DDS 1.4, 2.2.2.5).
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

namespace Omg.Dds.Sub;

/// <summary>SampleStateKind bits.</summary>
[Flags]
public enum SampleState : uint
{
    Read = 1,
    NotRead = 2,
    Any = 3,
}

/// <summary>ViewStateKind bits.</summary>
[Flags]
public enum ViewState : uint
{
    New = 1,
    NotNew = 2,
    Any = 3,
}

/// <summary>InstanceStateKind bits.</summary>
[Flags]
public enum InstanceState : uint
{
    Alive = 1,
    NotAliveDisposed = 2,
    NotAliveNoWriters = 4,
    NotAlive = 6,
    Any = 7,
}

/// <summary>The SampleInfo of a sample (DDS 1.4, 2.2.2.5.5).</summary>
public sealed record SampleInfo(
    SampleState SampleState,
    ViewState ViewState,
    InstanceState InstanceState,
    int DisposedGenerationCount,
    int NoWritersGenerationCount,
    int SampleRank,
    int GenerationRank,
    int AbsoluteGenerationRank,
    Time SourceTimestamp,
    InstanceHandle InstanceHandle,
    InstanceHandle PublicationHandle,
    long SequenceNumber,
    bool ValidData)
{
    internal static SampleInfo Of(in RddsSampleInfo i) => new(
        (SampleState)i.sample_state, (ViewState)i.view_state, (InstanceState)i.instance_state,
        i.disposed_generation_count, i.no_writers_generation_count, i.sample_rank, i.generation_rank,
        i.absolute_generation_rank, Time.Of(i.source_timestamp), InstanceHandle.Of(i.instance_handle),
        InstanceHandle.Of(i.publication_handle), i.sequence_number, i.valid_data != 0);
}

/// <summary>A sample: its data (default for a sample without data, as
/// after a dispose; Info.ValidData tells) and its SampleInfo.</summary>
public sealed record Sample<T>(T? Data, SampleInfo Info);

public sealed unsafe class Subscriber : Entity
{
    internal readonly SubscriberHandle handle;
    private readonly List<Entity> readers = new();

    internal Subscriber(DomainParticipant participant, SubscriberHandle handle, SubscriberQos qos)
    {
        Participant = participant;
        this.handle = handle;
        Qos = qos;
    }

    internal nint Raw => handle.Raw;

    public DomainParticipant Participant { get; }

    public SubscriberQos Qos { get; }

    public override InstanceHandle InstanceHandle => InstanceHandle.Nil;

    /// <summary>rustorb-dds's default reader QoS; change it with `with`.</summary>
    public DataReaderQos DefaultDataReaderQos => DataReaderQos.Default;

    /// <summary>A reader of a topic or of a content filtered topic.</summary>
    public DataReader<T> CreateDataReader<T>(ITopicDescription<T> topic, DataReaderQos? qos = null,
        IDataReaderListener<T>? listener = null, StatusKind mask = StatusKind.All)
    {
        qos ??= DataReaderQos.Default;
        ReaderListenerState? state = listener is null ? null : State(listener);
        ICodec<T> codec;
        nint r;
        try
        {
            switch (topic)
            {
                case Topic<T> t:
                    codec = t.codec;
                    r = qos.With(q => Create(state, mask, (l, m) =>
                    {
                        nint h;
                        Errors.Check(Ffi.rdds_subscriber_create_reader(Raw, t.Raw, q, (RddsReaderListener*)l, m, &h));
                        return h;
                    }));
                    GC.KeepAlive(t);
                    break;
                case ContentFilteredTopic<T> cf:
                    codec = cf.RelatedTopic.codec;
                    r = qos.With(q => Create(state, mask, (l, m) =>
                    {
                        nint h;
                        Errors.Check(Ffi.rdds_subscriber_create_filtered_reader(Raw, cf.Raw, q, (RddsReaderListener*)l, m, &h));
                        return h;
                    }));
                    GC.KeepAlive(cf);
                    break;
                default:
                    throw new ArgumentException("a topic description of another implementation");
            }
        }
        catch
        {
            state?.Free();
            throw;
        }
        GC.KeepAlive(this);
        var reader = new DataReader<T>(this, topic, codec, new ReaderHandle(r), qos);
        state?.Attach(reader);
        lock (readers)
        {
            readers.Add(reader);
        }
        return reader;
    }

    private static nint Create(ReaderListenerState? state, StatusKind mask, Func<nint, uint, nint> f)
    {
        if (state is null)
        {
            return f(0, (uint)mask);
        }
        RddsReaderListener table = ListenerTables.Reader(state);
        return f((nint)(&table), (uint)mask);
    }

    /// <summary>Sets the listener (null clears it).</summary>
    public void SetListener(ISubscriberListener? listener, StatusKind mask = StatusKind.All)
    {
        Participant.SetGroupListener(listener, mask, (l, m) => Ffi.rdds_subscriber_set_listener(Raw, (RddsListener*)l, m));
        GC.KeepAlive(this);
    }

    /// <summary>Calls OnDataAvailable of the listeners of the readers that
    /// have new data.</summary>
    public void NotifyDataReaders()
    {
        Errors.Check(Ffi.rdds_subscriber_notify_datareaders(Raw));
        GC.KeepAlive(this);
    }

    internal bool Same(nint other)
    {
        bool same = Ffi.rdds_subscriber_same(Raw, other) != 0;
        GC.KeepAlive(this);
        return same;
    }

    internal void Remove(Entity r)
    {
        lock (readers)
        {
            readers.Remove(r);
        }
    }

    public void DeleteContainedEntities()
    {
        List<Entity> all;
        lock (readers)
        {
            all = new List<Entity>(readers);
            readers.Clear();
        }
        foreach (Entity r in all)
        {
            r.Dispose();
        }
    }

    protected override void Close()
    {
        DeleteContainedEntities();
        Participant.Remove(this);
        handle.Dispose();
    }

    private sealed class ReaderState<T> : ReaderListenerState
    {
        private readonly IDataReaderListener<T> l;

        internal ReaderState(IDataReaderListener<T> l) => this.l = l;

        internal override void DeadlineMissed(RequestedDeadlineMissedStatus s) =>
            Deliver(r => l.OnRequestedDeadlineMissed((DataReader<T>)r, s));

        internal override void IncompatibleQos(RequestedIncompatibleQosStatus s) =>
            Deliver(r => l.OnRequestedIncompatibleQos((DataReader<T>)r, s));

        internal override void SampleRejected(SampleRejectedStatus s) =>
            Deliver(r => l.OnSampleRejected((DataReader<T>)r, s));

        internal override void LivelinessChanged(LivelinessChangedStatus s) =>
            Deliver(r => l.OnLivelinessChanged((DataReader<T>)r, s));

        internal override void DataAvailable() => Deliver(r => l.OnDataAvailable((DataReader<T>)r));

        internal override void SubscriptionMatched(SubscriptionMatchedStatus s) =>
            Deliver(r => l.OnSubscriptionMatched((DataReader<T>)r, s));

        internal override void SampleLost(SampleLostStatus s) => Deliver(r => l.OnSampleLost((DataReader<T>)r, s));
    }

    internal static ReaderListenerState State<T>(IDataReaderListener<T> l) => new ReaderState<T>(l);
}

/// <summary>A reader, whatever its type (for the listeners of subscribers
/// and participants).</summary>
public interface IDataReader : IDisposable
{
    InstanceHandle InstanceHandle { get; }
    ITopicDescription TopicDescription { get; }
    Subscriber Subscriber { get; }
    /// <summary>Waits until the reader has data (rustorb-dds).</summary>
    void WaitForData(Duration timeout);
    /// <summary>Waits until the reader matches count writers (rustorb-dds).</summary>
    void WaitForPublications(int count, Duration timeout);
}

/// <summary>A reader of T.</summary>
public sealed unsafe class DataReader<T> : Entity, IDataReader
{
    internal readonly ReaderHandle handle;
    private readonly ICodec<T> codec;
    private readonly InstanceHandle guid;
    private readonly object sync = new();

    internal DataReader(Subscriber subscriber, ITopicDescription<T> topic, ICodec<T> codec, ReaderHandle handle,
        DataReaderQos qos)
    {
        Subscriber = subscriber;
        TopicDescription = topic;
        this.codec = codec;
        this.handle = handle;
        Qos = qos;
        RddsGuid g;
        Errors.Check(Ffi.rdds_reader_guid(handle.Raw, &g));
        guid = InstanceHandle.Of(g);
        Endpoints.Add(guid, this);
    }

    internal nint Raw => handle.Raw;

    public Subscriber Subscriber { get; }

    public ITopicDescription<T> TopicDescription { get; }

    ITopicDescription IDataReader.TopicDescription => TopicDescription;

    public DataReaderQos Qos { get; }

    /// <summary>The reader's GUID.</summary>
    public override InstanceHandle InstanceHandle => guid;

    public const int LengthUnlimited = -1;

    /// <summary>Reads the samples in these states (they stay in the reader).</summary>
    public IList<Sample<T>> Read(int maxSamples = LengthUnlimited, SampleState sampleStates = SampleState.Any,
        ViewState viewStates = ViewState.Any, InstanceState instanceStates = InstanceState.Any) =>
        Select(false, maxSamples, sampleStates, viewStates, instanceStates, 0, null);

    /// <summary>Takes the samples in these states (they leave the reader).</summary>
    public IList<Sample<T>> Take(int maxSamples = LengthUnlimited, SampleState sampleStates = SampleState.Any,
        ViewState viewStates = ViewState.Any, InstanceState instanceStates = InstanceState.Any) =>
        Select(true, maxSamples, sampleStates, viewStates, instanceStates, 0, null);

    /// <summary>Reads the samples of one instance.</summary>
    public IList<Sample<T>> ReadInstance(InstanceHandle instance, int maxSamples = LengthUnlimited,
        SampleState sampleStates = SampleState.Any, ViewState viewStates = ViewState.Any,
        InstanceState instanceStates = InstanceState.Any) =>
        Select(false, maxSamples, sampleStates, viewStates, instanceStates, 1, instance);

    /// <summary>Takes the samples of one instance.</summary>
    public IList<Sample<T>> TakeInstance(InstanceHandle instance, int maxSamples = LengthUnlimited,
        SampleState sampleStates = SampleState.Any, ViewState viewStates = ViewState.Any,
        InstanceState instanceStates = InstanceState.Any) =>
        Select(true, maxSamples, sampleStates, viewStates, instanceStates, 1, instance);

    /// <summary>Reads the samples of the instance after this one (Nil: the first).</summary>
    public IList<Sample<T>> ReadNextInstance(InstanceHandle previous, int maxSamples = LengthUnlimited,
        SampleState sampleStates = SampleState.Any, ViewState viewStates = ViewState.Any,
        InstanceState instanceStates = InstanceState.Any) =>
        Select(false, maxSamples, sampleStates, viewStates, instanceStates, 2, previous);

    /// <summary>Takes the samples of the instance after this one (Nil: the first).</summary>
    public IList<Sample<T>> TakeNextInstance(InstanceHandle previous, int maxSamples = LengthUnlimited,
        SampleState sampleStates = SampleState.Any, ViewState viewStates = ViewState.Any,
        InstanceState instanceStates = InstanceState.Any) =>
        Select(true, maxSamples, sampleStates, viewStates, instanceStates, 2, previous);

    /// <summary>Reads the samples of a read condition of this reader.</summary>
    public IList<Sample<T>> Read(ReadCondition condition, int maxSamples = LengthUnlimited) =>
        SelectWithCondition(false, condition, maxSamples);

    /// <summary>Takes the samples of a read condition of this reader.</summary>
    public IList<Sample<T>> Take(ReadCondition condition, int maxSamples = LengthUnlimited) =>
        SelectWithCondition(true, condition, maxSamples);

    private IList<Sample<T>> Select(bool take, int maxSamples, SampleState s, ViewState v, InstanceState i,
        int mode, InstanceHandle? instance)
    {
        nint samples;
        RddsInstanceHandle h = instance?.Native ?? default;
        Errors.Check(Ffi.rdds_reader_select(Raw, (byte)(take ? 1 : 0), maxSamples, (uint)s, (uint)v, (uint)i, mode,
            instance is null ? null : &h, &samples));
        GC.KeepAlive(this);
        return Samples(samples);
    }

    private IList<Sample<T>> SelectWithCondition(bool take, ReadCondition condition, int maxSamples)
    {
        nint samples;
        Errors.Check(Ffi.rdds_reader_select_w_condition(Raw, (byte)(take ? 1 : 0), maxSamples, condition.Raw,
            &samples));
        GC.KeepAlive(this);
        GC.KeepAlive(condition);
        return Samples(samples);
    }

    private IList<Sample<T>> Samples(nint samples)
    {
        using var s = new SamplesHandle(samples);
        int n = (int)Ffi.rdds_samples_count(samples);
        var list = new List<Sample<T>>(n);
        for (int k = 0; k < n; k++)
        {
            RddsSampleInfo info;
            Errors.Check(Ffi.rdds_samples_info(samples, (nuint)k, &info));
            T? data = default;
            nint input;
            int rc = Ffi.rdds_samples_data(samples, (nuint)k, &input);
            if (rc == Status.OK)
            {
                using var i = new In(input);
                data = codec.Read(i);
                i.Check();
            }
            else if (rc != Status.NO_DATA)
            {
                Errors.Check(rc);
            }
            list.Add(new Sample<T>(data, SampleInfo.Of(info)));
        }
        return list;
    }

    /// <summary>A sample with the key of an instance (its other members
    /// as in a new value).</summary>
    public T GetKeyValue(InstanceHandle instance)
    {
        nint input;
        Errors.Check(Ffi.rdds_reader_get_key_value(Raw, instance.Native, &input));
        GC.KeepAlive(this);
        using var i = new In(input);
        T v = codec.Read(i);
        i.Check();
        return v;
    }

    /// <summary>The handle of the sample's instance; Nil when the reader does
    /// not know it.</summary>
    public InstanceHandle LookupInstance(T instance)
    {
        if (instance is null)
        {
            throw new ArgumentNullException(nameof(instance));
        }
        using var o = new Out();
        codec.Write(o, instance);
        RddsInstanceHandle h;
        Errors.Check(Ffi.rdds_reader_lookup_instance(Raw, o.Raw, &h));
        GC.KeepAlive(this);
        return InstanceHandle.Of(h);
    }

    /// <summary>Until a sample is there; TimeoutException (rustorb's extension).</summary>
    public void WaitForData(Duration timeout)
    {
        Errors.Check(Ffi.rdds_reader_wait_for_data(Raw, timeout.Native));
        GC.KeepAlive(this);
    }

    /// <summary>Until count writers match; TimeoutException (rustorb's extension).</summary>
    public void WaitForPublications(int count, Duration timeout)
    {
        Errors.Check(Ffi.rdds_reader_wait_for_publications(Raw, (nuint)count, timeout.Native));
        GC.KeepAlive(this);
    }

    /// <summary>Returns at once: TRANSIENT_LOCAL data comes with the match.</summary>
    public void WaitForHistoricalData(Duration maxWait) { }

    /// <summary>The GUIDs of the matched writers.</summary>
    public IReadOnlyList<InstanceHandle> GetMatchedPublications()
    {
        nuint n;
        Errors.Check(Ffi.rdds_reader_matched_publications(Raw, null, 0, &n));
        var buf = new RddsGuid[(int)n + 8];
        fixed (RddsGuid* p = buf)
        {
            Errors.Check(Ffi.rdds_reader_matched_publications(Raw, p, (nuint)buf.Length, &n));
        }
        GC.KeepAlive(this);
        var r = new List<InstanceHandle>();
        for (int i = 0; i < (int)Math.Min(n, (nuint)buf.Length); i++)
        {
            r.Add(InstanceHandle.Of(buf[i]));
        }
        return r;
    }

    public SubscriptionMatchedStatus GetSubscriptionMatchedStatus()
    {
        RddsMatchedStatus s;
        Errors.Check(Ffi.rdds_reader_subscription_matched_status(Raw, &s));
        GC.KeepAlive(this);
        return SubscriptionMatchedStatus.Of(s);
    }

    public RequestedIncompatibleQosStatus GetRequestedIncompatibleQosStatus()
    {
        RddsIncompatibleQosStatus s;
        Errors.Check(Ffi.rdds_reader_requested_incompatible_qos_status(Raw, &s));
        GC.KeepAlive(this);
        return RequestedIncompatibleQosStatus.Of(s);
    }

    public LivelinessChangedStatus GetLivelinessChangedStatus()
    {
        RddsLivelinessChangedStatus s;
        Errors.Check(Ffi.rdds_reader_liveliness_changed_status(Raw, &s));
        GC.KeepAlive(this);
        return LivelinessChangedStatus.Of(s);
    }

    public RequestedDeadlineMissedStatus GetRequestedDeadlineMissedStatus()
    {
        RddsDeadlineMissedStatus s;
        Errors.Check(Ffi.rdds_reader_requested_deadline_missed_status(Raw, &s));
        GC.KeepAlive(this);
        return RequestedDeadlineMissedStatus.Of(s);
    }

    public SampleLostStatus GetSampleLostStatus()
    {
        RddsCountStatus s;
        Errors.Check(Ffi.rdds_reader_sample_lost_status(Raw, &s));
        GC.KeepAlive(this);
        return SampleLostStatus.Of(s);
    }

    public SampleRejectedStatus GetSampleRejectedStatus()
    {
        RddsSampleRejectedStatus s;
        Errors.Check(Ffi.rdds_reader_sample_rejected_status(Raw, &s));
        GC.KeepAlive(this);
        return SampleRejectedStatus.Of(s);
    }

    /// <summary>The statuses that changed since they were last read.</summary>
    public StatusKind StatusChanges
    {
        get
        {
            uint m = Ffi.rdds_reader_status_changes(Raw);
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
                    Errors.Check(Ffi.rdds_reader_status_condition(Raw, &c));
                    GC.KeepAlive(this);
                    statusCondition = new StatusCondition(new ConditionHandle(c), this);
                }
                return statusCondition;
            }
        }
    }

    /// <summary>A condition true while the reader has samples in these states.</summary>
    public ReadCondition CreateReadCondition(SampleState sampleStates = SampleState.Any,
        ViewState viewStates = ViewState.Any, InstanceState instanceStates = InstanceState.Any)
    {
        nint c;
        Errors.Check(Ffi.rdds_reader_create_readcondition(Raw, (uint)sampleStates, (uint)viewStates,
            (uint)instanceStates, &c));
        GC.KeepAlive(this);
        return new ReadCondition(new ConditionHandle(c), this, sampleStates, viewStates, instanceStates);
    }

    /// <summary>Sets the listener (null clears it).</summary>
    public void SetListener(IDataReaderListener<T>? listener, StatusKind mask = StatusKind.All)
    {
        if (listener is null)
        {
            Errors.Check(Ffi.rdds_reader_set_listener(Raw, null, (uint)mask));
            GC.KeepAlive(this);
            return;
        }
        ReaderListenerState state = Subscriber.State(listener);
        state.Attach(this);
        RddsReaderListener table = ListenerTables.Reader(state);
        int rc = Ffi.rdds_reader_set_listener(Raw, &table, (uint)mask);
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
        Subscriber.Remove(this);
        handle.Dispose();
    }
}

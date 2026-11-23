// DomainParticipant (DDS 1.4, 2.2.2.2.1).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections.Generic;
using Omg.Dds.Core;
using Omg.Dds.Core.Status;
using Omg.Dds.Pub;
using Omg.Dds.Sub;
using Omg.Dds.Topic;
using Rustorb.Dds;
using Rustorb.Dds.Native;

namespace Omg.Dds.Domain;

/// <summary>A participant of a domain. Dispose deletes it and the entities
/// it contains.</summary>
public sealed unsafe class DomainParticipant : Entity
{
    internal readonly ParticipantHandle handle;
    private readonly List<Entity> contained = new();
    private readonly Dictionary<string, ITopicDescription> topics = new();

    /// <summary>A participant of the domain.</summary>
    public DomainParticipant(int domainId, DomainParticipantQos? qos = null,
        IDomainParticipantListener? listener = null, StatusKind mask = StatusKind.All)
        : this(domainId, qos, 0)
    {
        if (listener != null)
        {
            SetListener(listener, mask);
        }
    }

    internal DomainParticipant(int domainId, DomainParticipantQos? qos, uint flags)
    {
        Library.Check();
        if (domainId < 0)
        {
            throw new ArgumentException("a negative domain id");
        }
        Qos = qos ?? DomainParticipantQos.Default;
        nint h = QosNative.Participant(Qos, q =>
        {
            nint p;
            Errors.Check(Ffi.rdds_participant_new((uint)domainId, q, flags, &p));
            return p;
        });
        handle = new ParticipantHandle(h);
    }

    internal nint Raw => handle.Raw;

    public DomainParticipantQos Qos { get; }

    public int DomainId
    {
        get
        {
            int d = (int)Ffi.rdds_participant_domain_id(Raw);
            GC.KeepAlive(this);
            return d;
        }
    }

    /// <summary>The participant's GUID.</summary>
    public override InstanceHandle InstanceHandle
    {
        get
        {
            RddsGuid g;
            Errors.Check(Ffi.rdds_participant_guid(Raw, &g));
            GC.KeepAlive(this);
            return InstanceHandle.Of(g);
        }
    }

    public Time CurrentTime => Time.Now();

    public void AssertLiveliness()
    {
        Errors.Check(Ffi.rdds_participant_assert_liveliness(Raw));
        GC.KeepAlive(this);
    }

    /// <summary>How many other participants are known (rustorb's extension).</summary>
    public int DiscoveredParticipants
    {
        get
        {
            int n = (int)Ffi.rdds_participant_discovered_participants(Raw);
            GC.KeepAlive(this);
            return n;
        }
    }

    /// <summary>A topic of T, registered under T's IDL name.</summary>
    public Topic<T> CreateTopic<T>(string name, ITopicListener? listener = null, StatusKind mask = StatusKind.All)
    {
        ICodec<T> codec = TypeSupport.Get<T>();
        TypeRef type = codec.Type;
        nint t;
        fixed (byte* n = Utf8.Z(name))
        {
            Errors.Check(Ffi.rdds_participant_create_topic(Raw, n, type.Raw, &t));
        }
        GC.KeepAlive(type);
        var topic = new Topic<T>(this, new TopicHandle(t), name, codec);
        Add(topic);
        lock (topics)
        {
            topics[name] = topic;
        }
        if (listener != null)
        {
            topic.SetListener(listener, mask);
        }
        return topic;
    }

    /// <summary>A content filtered topic of a topic of this participant
    /// (DDS 1.4, 2.2.2.3.3): its readers get only the samples the filter
    /// expression (with %0, %1... for the parameters) selects.</summary>
    public ContentFilteredTopic<T> CreateContentFilteredTopic<T>(string name, Topic<T> related,
        string filterExpression, params string[] expressionParameters)
    {
        nint t;
        using (var parameters = new Utf8Array(expressionParameters))
        {
            fixed (byte* n = Utf8.Z(name), e = Utf8.Z(filterExpression))
            {
                Errors.Check(Ffi.rdds_participant_create_contentfilteredtopic(Raw, n, related.Raw, e,
                    parameters.Pointer, (nuint)expressionParameters.Length, &t));
            }
        }
        GC.KeepAlive(related);
        var topic = new ContentFilteredTopic<T>(this, new CfTopicHandle(t), name, related, filterExpression,
            expressionParameters);
        Add(topic);
        lock (topics)
        {
            topics[name] = topic;
        }
        return topic;
    }

    /// <summary>The topic or content filtered topic of this name this
    /// participant made, or null.</summary>
    public ITopicDescription? LookupTopicDescription(string name)
    {
        lock (topics)
        {
            return topics.TryGetValue(name, out ITopicDescription? t) ? t : null;
        }
    }

    public Publisher CreatePublisher(PublisherQos? qos = null, IPublisherListener? listener = null,
        StatusKind mask = StatusKind.All)
    {
        qos ??= PublisherQos.Default;
        nint p = QosNative.Group(qos.Partition, q =>
        {
            nint h;
            Errors.Check(Ffi.rdds_participant_create_publisher(Raw, q, &h));
            return h;
        });
        GC.KeepAlive(this);
        var publisher = new Publisher(this, new PublisherHandle(p), qos);
        Add(publisher);
        if (listener != null)
        {
            publisher.SetListener(listener, mask);
        }
        return publisher;
    }

    public Subscriber CreateSubscriber(SubscriberQos? qos = null, ISubscriberListener? listener = null,
        StatusKind mask = StatusKind.All)
    {
        qos ??= SubscriberQos.Default;
        nint s = QosNative.Group(qos.Partition, q =>
        {
            nint h;
            Errors.Check(Ffi.rdds_participant_create_subscriber(Raw, q, &h));
            return h;
        });
        GC.KeepAlive(this);
        var subscriber = new Subscriber(this, new SubscriberHandle(s), qos);
        Add(subscriber);
        if (listener != null)
        {
            subscriber.SetListener(listener, mask);
        }
        return subscriber;
    }

    /// <summary>Sets the listener (null clears it) and the statuses it takes.</summary>
    public void SetListener(IDomainParticipantListener? listener, StatusKind mask = StatusKind.All)
    {
        SetGroupListener(listener, mask, (l, m) => Ffi.rdds_participant_set_listener(Raw, (RddsListener*)l, m));
    }

    internal void SetGroupListener(object? listener, StatusKind mask, Func<nint, uint, int> set)
    {
        if (listener is null)
        {
            Errors.Check(set(0, (uint)mask));
            GC.KeepAlive(this);
            return;
        }
        var state = new GroupState(listener, this);
        RddsListener table = ListenerTables.Group(state);
        int rc = set((nint)(&table), (uint)mask);
        GC.KeepAlive(this);
        if (rc != Status.OK)
        {
            state.Free();
            Errors.Check(rc);
        }
    }

    internal void Add(Entity e)
    {
        lock (contained)
        {
            contained.Add(e);
        }
    }

    internal void Remove(Entity e)
    {
        lock (contained)
        {
            contained.Remove(e);
        }
        if (e is ITopicDescription t)
        {
            lock (topics)
            {
                if (topics.TryGetValue(t.Name, out ITopicDescription? x) && ReferenceEquals(x, e))
                {
                    topics.Remove(t.Name);
                }
            }
        }
    }

    internal IEnumerable<Subscriber> Subscribers()
    {
        lock (contained)
        {
            return contained.FindAll(e => e is Subscriber).ConvertAll(e => (Subscriber)e);
        }
    }

    /// <summary>Deletes the entities this participant contains.</summary>
    public void DeleteContainedEntities()
    {
        List<Entity> all;
        lock (contained)
        {
            all = new List<Entity>(contained);
            contained.Clear();
        }
        // Readers and writers first (inside publishers and subscribers),
        // then the topics.
        foreach (Entity e in all)
        {
            if (e is Publisher or Subscriber)
            {
                e.Dispose();
            }
        }
        foreach (Entity e in all)
        {
            e.Dispose();
        }
        lock (topics)
        {
            topics.Clear();
        }
    }

    protected override void Close()
    {
        DeleteContainedEntities();
        handle.Dispose();
    }

    /// <summary>The listener state of rdds_listener: finds the endpoint of
    /// a status by its GUID.</summary>
    private sealed class GroupState : GroupListenerState
    {
        private readonly object listener;
        private readonly DomainParticipant participant;

        internal GroupState(object listener, DomainParticipant participant)
        {
            this.listener = listener;
            this.participant = participant;
            Attach(participant);
        }

        internal override void WriterStatus(InstanceHandle guid, Action<object, IPublisherListener> call)
        {
            if (listener is IPublisherListener l)
            {
                Deliver(_ => Endpoints.Deliver(guid, w => call(w, l)));
            }
        }

        internal override void ReaderStatus(InstanceHandle guid, Action<object, ISubscriberListener> call)
        {
            if (listener is ISubscriberListener l)
            {
                Deliver(_ => Endpoints.Deliver(guid, r => call(r, l)));
            }
        }

        internal override void DataOnReaders(nint subscriber)
        {
            if (listener is not ISubscriberListener l)
            {
                return;
            }
            foreach (Subscriber s in participant.Subscribers())
            {
                if (s.Same(subscriber))
                {
                    Deliver(_ => l.OnDataOnReaders(s));
                    return;
                }
            }
        }

        internal override void InconsistentTopic(string name, InconsistentTopicStatus status)
        {
            if (listener is ITopicListener l && participant.LookupTopicDescription(name) is ITopic t)
            {
                Deliver(_ => l.OnInconsistentTopic(t, status));
            }
        }
    }
}

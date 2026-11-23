// The entities of the .NET binding on a loopback-only domain: writers and
// readers, keyed instances, listeners, conditions and wait sets, QoS and
// statuses.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.IO;
using System.Linq;
using System.Threading;
using HelloWorld;
using Keyed;
using Omg.Dds.Core;
using Omg.Dds.Core.Policy;
using Omg.Dds.Core.Status;
using Omg.Dds.Domain;
using Omg.Dds.Pub;
using Omg.Dds.Sub;
using Omg.Dds.Topic;
using Xunit;

namespace Rustorb.Dds.Tests;

public sealed class EntitiesTests : IDisposable
{
    // Domains of their own, so that other tests running at the same time do
    // not match (ports from 49152 up can be reserved on Windows).
    private static int domain = 60;

    private static readonly Duration TenSeconds = TimeSpan.FromSeconds(10);

    private readonly int domainId;
    private readonly DomainParticipant dp;

    public EntitiesTests()
    {
        domainId = Interlocked.Increment(ref domain);
        dp = Participants.Localhost(domainId);
    }

    public void Dispose() => dp.Dispose();

    private static DataWriterQos ReliableWriter() =>
        DataWriterQos.Default with { Reliability = Reliability.Reliable(), History = History.KeepAll() };

    private static DataReaderQos ReliableReader() =>
        DataReaderQos.Default with { Reliability = Reliability.Reliable(), History = History.KeepAll() };

    private static void Eventually(Func<bool> f)
    {
        DateTime end = DateTime.UtcNow.AddSeconds(10);
        while (!f())
        {
            Assert.True(DateTime.UtcNow < end, "timed out");
            Thread.Sleep(10);
        }
    }

    [Fact]
    public void Participant()
    {
        Assert.Equal(domainId, dp.DomainId);
        Assert.True(dp.CurrentTime.Sec > 0);
        Assert.NotEqual(InstanceHandle.Nil, dp.InstanceHandle);
        Assert.True(dp.DiscoveredParticipants >= 0);
        dp.AssertLiveliness();
        Assert.Equal(new Duration(1, 500_000_000), (Duration)TimeSpan.FromMilliseconds(1500));
        Assert.Equal(TimeSpan.FromMilliseconds(1500), new Duration(1, 500_000_000).ToTimeSpan());
    }

    private sealed class ParticipantListener : IDomainParticipantListener
    {
        public readonly ConcurrentQueue<string> Calls = new();
        public readonly ManualResetEventSlim Published = new();

        public void OnPublicationMatched(IDataWriter writer, PublicationMatchedStatus status)
        {
            Calls.Enqueue("participant:matched " + writer.Topic.Name);
            Published.Set();
        }

        public void OnDataAvailable(IDataReader reader) => Calls.Enqueue("participant:data_available");
    }

    private sealed class SubscriberListener : ISubscriberListener
    {
        public readonly ConcurrentQueue<string> Calls;
        public readonly ManualResetEventSlim OnReaders = new();

        public SubscriberListener(ConcurrentQueue<string> calls) => Calls = calls;

        public void OnDataOnReaders(Subscriber subscriber)
        {
            Calls.Enqueue("subscriber:data_on_readers");
            subscriber.NotifyDataReaders();
            OnReaders.Set();
        }
    }

    private sealed class ReaderListener<T> : IDataReaderListener<T>
    {
        public readonly ConcurrentQueue<string> Calls;
        public readonly ManualResetEventSlim Data = new();

        public ReaderListener(ConcurrentQueue<string> calls) => Calls = calls;

        public void OnDataAvailable(DataReader<T> reader)
        {
            Calls.Enqueue("reader:data_available");
            Data.Set();
        }
    }

    [Fact]
    public void GroupListeners()
    {
        var pl = new ParticipantListener();
        dp.SetListener(pl, StatusKind.PublicationMatched | StatusKind.DataAvailable);
        Topic<Greeting> topic = dp.CreateTopic<Greeting>("grouped");
        Publisher pub = dp.CreatePublisher();
        var sl = new SubscriberListener(pl.Calls);
        Subscriber sub = dp.CreateSubscriber(null, sl, StatusKind.DataOnReaders);
        var rl = new ReaderListener<Greeting>(pl.Calls);
        DataReader<Greeting> reader = sub.CreateDataReader(topic, ReliableReader(), rl, StatusKind.DataAvailable);
        DataWriter<Greeting> writer = pub.CreateDataWriter(topic, ReliableWriter());
        Assert.True(pl.Published.Wait(TimeSpan.FromSeconds(10)));
        Assert.Contains("participant:matched grouped", pl.Calls);
        writer.Write(new Greeting(1, "g"));
        Assert.True(sl.OnReaders.Wait(TimeSpan.FromSeconds(10)));
        Assert.True(rl.Data.Wait(TimeSpan.FromSeconds(10)));
        Assert.DoesNotContain("participant:data_available", pl.Calls);
        Assert.Single(reader.Take());
        dp.SetListener(null);
    }

    [Fact]
    public void ContentFilteredTopic()
    {
        Topic<Greeting> topic = dp.CreateTopic<Greeting>("filtered_greetings");
        ContentFilteredTopic<Greeting> cft = dp.CreateContentFilteredTopic("big_greetings", topic,
            "index > %0 AND message LIKE 'h%'", "3");
        Assert.Equal("big_greetings", cft.Name);
        Assert.Equal("HelloWorld::Greeting", cft.TypeName);
        Assert.Same(topic, cft.RelatedTopic);
        Assert.Same(cft, dp.LookupTopicDescription("big_greetings"));
        Assert.Equal(new[] { "3" }, cft.ExpressionParameters);
        Publisher pub = dp.CreatePublisher();
        Subscriber sub = dp.CreateSubscriber();
        DataWriter<Greeting> writer = pub.CreateDataWriter(topic, ReliableWriter());
        DataReader<Greeting> reader = sub.CreateDataReader(cft, ReliableReader());
        Assert.Same(cft, reader.TopicDescription);
        writer.WaitForSubscriptions(1, TenSeconds);
        for (uint i = 1; i <= 6; i++)
        {
            writer.Write(new Greeting(i, i == 5 ? "bye" : "hello"));
        }
        writer.WaitForAcknowledgments(TenSeconds);
        var got = new List<uint>();
        Eventually(() =>
        {
            got.AddRange(reader.Take().Select(s => s.Data!.index));
            return got.Count >= 2;
        });
        Assert.Equal(new uint[] { 4, 6 }, got);
        cft.ExpressionParameters = new[] { "5" };
        Assert.Equal(new[] { "5" }, cft.ExpressionParameters);
        Assert.ThrowsAny<Exception>(() => cft.ExpressionParameters = Array.Empty<string>());
        Assert.ThrowsAny<Exception>(() => dp.CreateContentFilteredTopic("bad", topic, "nope = 1"));
    }

    [Fact]
    public void TransientSamplesOutliveTheWriter()
    {
        DataWriterQos defaults = DataWriterQos.Default;
        Assert.Equal(1, defaults.DurabilityService.HistoryDepth);
        Assert.Equal(-1, defaults.DurabilityService.MaxSamples);
        Topic<Greeting> topic = dp.CreateTopic<Greeting>("kept");
        Publisher pub = dp.CreatePublisher();
        DataWriter<Greeting> writer = pub.CreateDataWriter(topic, ReliableWriter() with
        {
            Durability = Durability.Transient(),
            History = History.KeepLast(3),
            DurabilityService = defaults.DurabilityService with { HistoryDepth = 2 },
        });
        for (uint i = 1; i <= 3; i++)
        {
            writer.Write(new Greeting(i, "kept " + i));
        }
        writer.Dispose();
        Subscriber sub = dp.CreateSubscriber();
        DataReader<Greeting> reader = sub.CreateDataReader(topic,
            ReliableReader() with { Durability = Durability.Transient() });
        var got = new List<uint>();
        Eventually(() =>
        {
            got.AddRange(reader.Take().Select(s => s.Data!.index));
            return got.Count >= 2;
        });
        Assert.Equal(new uint[] { 2, 3 }, got);
    }

    [Fact]
    public void WriteAndTake()
    {
        Topic<Greeting> topic = dp.CreateTopic<Greeting>("greetings");
        Assert.Equal("greetings", topic.Name);
        Assert.Equal("HelloWorld::Greeting", topic.TypeName);
        Assert.Same(topic, dp.LookupTopicDescription("greetings"));
        Publisher pub = dp.CreatePublisher();
        Subscriber sub = dp.CreateSubscriber();
        DataWriter<Greeting> writer = pub.CreateDataWriter(topic, ReliableWriter());
        DataReader<Greeting> reader = sub.CreateDataReader(topic, ReliableReader());
        writer.WaitForSubscriptions(1, TenSeconds);
        reader.WaitForPublications(1, TenSeconds);
        for (uint i = 1; i <= 3; i++)
        {
            writer.Write(new Greeting(i, "hello " + i));
        }
        writer.WaitForAcknowledgments(TenSeconds);
        reader.WaitForData(TenSeconds);

        // Read leaves them, and marks them read.
        IList<Sample<Greeting>> read = reader.Read();
        Assert.Equal(3, read.Count);
        Assert.Equal(SampleState.NotRead, read[0].Info.SampleState);
        Assert.Equal(ViewState.New, read[0].Info.ViewState);
        Assert.Equal(InstanceState.Alive, read[0].Info.InstanceState);
        Assert.True(read[0].Info.SourceTimestamp.Sec > 0);
        Assert.Equal(SampleState.Read, reader.Read()[0].Info.SampleState);
        Assert.Equal(2, reader.Read(2).Count);
        // Take with max samples: the first to arrive.
        IList<Sample<Greeting>> one = reader.Take(1);
        Assert.Equal(1u, Assert.Single(one).Data!.index);
        IList<Sample<Greeting>> taken = reader.Take();
        Assert.Equal(2, taken.Count);
        for (int i = 0; i < 2; i++)
        {
            Assert.Equal((uint)(i + 2), taken[i].Data!.index);
            Assert.Equal("hello " + (i + 2), taken[i].Data!.message);
        }
        Assert.Empty(reader.Take());

        Assert.Equal(1, writer.GetPublicationMatchedStatus().CurrentCount);
        Assert.Equal(1, reader.GetSubscriptionMatchedStatus().CurrentCount);
        Assert.Single(writer.GetMatchedSubscriptions());
        Assert.Single(reader.GetMatchedPublications());
        Assert.Same(pub, writer.Publisher);
        Assert.Same(topic, writer.Topic);
        Assert.Same(sub, reader.Subscriber);
    }

    [Fact]
    public void Qos()
    {
        Topic<Greeting> topic = dp.CreateTopic<Greeting>("qos");
        Publisher pub = dp.CreatePublisher(new PublisherQos(new Partition(ImmutableArray.Create("a", "b"))));
        Assert.Equal(new[] { "a", "b" }, pub.Qos.Partition.Name);
        DataWriter<Greeting> writer = pub.CreateDataWriter(topic, DataWriterQos.Default with
        {
            Reliability = Reliability.Reliable(),
            History = History.KeepLast(7),
            Durability = Durability.TransientLocal(),
            Deadline = new Deadline(TimeSpan.FromSeconds(2)),
            UserData = new UserData(ImmutableArray.Create<byte>(1, 2, 3)),
        });
        DataWriterQos q = writer.Qos;
        Assert.Equal(ReliabilityKind.Reliable, q.Reliability.Kind);
        Assert.Equal(7, q.History.Depth);
        Assert.Equal(TimeSpan.FromSeconds(2), q.Deadline.Period.ToTimeSpan());
        Subscriber sub = dp.CreateSubscriber();
        DataReader<Greeting> reader = sub.CreateDataReader(topic);
        Assert.Equal(ReliabilityKind.BestEffort, reader.Qos.Reliability.Kind);
        Assert.Equal(DataReaderQos.Default, DataReaderQos.Default);
    }

    [Fact]
    public void KeyedInstances()
    {
        Topic<Shape> topic = dp.CreateTopic<Shape>("shapes");
        Publisher pub = dp.CreatePublisher();
        Subscriber sub = dp.CreateSubscriber();
        DataWriter<Shape> writer = pub.CreateDataWriter(topic, ReliableWriter() with
        {
            WriterDataLifecycle = new WriterDataLifecycle(false),
        });
        DataReader<Shape> reader = sub.CreateDataReader(topic, ReliableReader());
        writer.WaitForSubscriptions(1, TenSeconds);

        InstanceHandle two = writer.RegisterInstance(new Shape(2, 0, ""));
        Assert.NotEqual(InstanceHandle.Nil, two);
        Assert.Equal(two, writer.LookupInstance(new Shape(2, 99, "other")));
        Assert.Equal(InstanceHandle.Nil, writer.LookupInstance(new Shape(5, 0, "")));
        for (uint i = 1; i <= 6; i++)
        {
            writer.Write(new Shape((i - 1) % 3 + 1, i, "s"));
        }
        writer.DisposeInstance(new Shape(2, 0, ""));
        writer.UnregisterInstance(new Shape(3, 0, ""));
        writer.WaitForAcknowledgments(TenSeconds);

        InstanceHandle readerTwo = reader.LookupInstance(new Shape(2, 0, ""));
        Assert.NotEqual(InstanceHandle.Nil, readerTwo);
        Eventually(() => reader.Read().Count >= 8);
        Assert.Equal(2u, reader.GetKeyValue(readerTwo).id);
        // The samples of one instance.
        IList<Sample<Shape>> ofTwo = reader.ReadInstance(readerTwo);
        Assert.True(ofTwo.Count >= 2);
        Assert.All(ofTwo, s => Assert.Equal(readerTwo, s.Info.InstanceHandle));
        bool disposed = false;
        bool noWriters = false;
        int valid = 0;
        foreach (Sample<Shape> s in reader.Take())
        {
            if (s.Info.ValidData)
            {
                valid++;
            }
            else if (s.Info.InstanceHandle == readerTwo)
            {
                disposed |= s.Info.InstanceState == InstanceState.NotAliveDisposed;
            }
            else
            {
                noWriters |= s.Info.InstanceState == InstanceState.NotAliveNoWriters;
            }
        }
        Assert.Equal(6, valid);
        Assert.True(disposed);
        Assert.True(noWriters);
    }

    private sealed class WriterListener : IDataWriterListener<Greeting>
    {
        public readonly ManualResetEventSlim Matched = new();

        public void OnPublicationMatched(DataWriter<Greeting> writer, PublicationMatchedStatus status)
        {
            if (status.CurrentCount == 1)
            {
                Matched.Set();
            }
        }
    }

    private sealed class TakingListener : IDataReaderListener<Greeting>
    {
        public readonly ManualResetEventSlim Matched = new();
        public readonly CountdownEvent Data = new(3);
        public readonly ConcurrentQueue<uint> Got = new();

        public void OnSubscriptionMatched(DataReader<Greeting> reader, SubscriptionMatchedStatus status) =>
            Matched.Set();

        public void OnDataAvailable(DataReader<Greeting> reader)
        {
            foreach (Sample<Greeting> s in reader.Take())
            {
                Got.Enqueue(s.Data!.index);
                if (Data.CurrentCount > 0)
                {
                    Data.Signal();
                }
            }
        }
    }

    [Fact]
    public void Listeners()
    {
        Topic<Greeting> topic = dp.CreateTopic<Greeting>("listened");
        Publisher pub = dp.CreatePublisher();
        Subscriber sub = dp.CreateSubscriber();
        var wl = new WriterListener();
        DataWriter<Greeting> writer = pub.CreateDataWriter(topic, ReliableWriter(), wl);
        var rl = new TakingListener();
        DataReader<Greeting> reader = sub.CreateDataReader(topic, ReliableReader(), rl);
        Assert.True(rl.Matched.Wait(TimeSpan.FromSeconds(10)));
        Assert.True(wl.Matched.Wait(TimeSpan.FromSeconds(10)));
        for (uint i = 1; i <= 3; i++)
        {
            writer.Write(new Greeting(i, "x"));
        }
        Assert.True(rl.Data.Wait(TimeSpan.FromSeconds(10)));
        Assert.Equal(new uint[] { 1, 2, 3 }, rl.Got);
        // Without a listener, no more calls.
        reader.SetListener(null);
        writer.Write(new Greeting(4, "x"));
        reader.WaitForData(TenSeconds);
        Assert.Equal(3, rl.Got.Count);
    }

    [Fact]
    public void ConditionsAndWaitSets()
    {
        Topic<Greeting> topic = dp.CreateTopic<Greeting>("waited");
        Publisher pub = dp.CreatePublisher();
        Subscriber sub = dp.CreateSubscriber();
        DataWriter<Greeting> writer = pub.CreateDataWriter(topic, ReliableWriter());
        DataReader<Greeting> reader = sub.CreateDataReader(topic, ReliableReader());
        using var ws = new WaitSet();

        // A guard condition.
        using var guard = new GuardCondition();
        ws.AttachCondition(guard);
        Assert.Throws<TimeoutException>(() => ws.Wait(TimeSpan.FromMilliseconds(50)));
        guard.TriggerValue = true;
        Assert.Same(guard, Assert.Single(ws.Wait(TenSeconds)));
        ws.DetachCondition(guard);

        // A status condition.
        StatusCondition status = reader.StatusCondition;
        status.EnabledStatuses = StatusKind.DataAvailable;
        Assert.Same(reader, status.Entity);
        ws.AttachCondition(status);
        writer.WaitForSubscriptions(1, TenSeconds);
        writer.Write(new Greeting(1, "x"));
        Assert.Same(status, Assert.Single(ws.Wait(TenSeconds)));
        Assert.True(reader.StatusChanges.HasFlag(StatusKind.DataAvailable));
        ws.DetachCondition(status);

        // A read condition on samples not read.
        using ReadCondition notRead = reader.CreateReadCondition(SampleState.NotRead);
        ws.AttachCondition(notRead);
        Assert.Same(notRead, Assert.Single(ws.Wait(TenSeconds)));
        Assert.True(notRead.TriggerValue);
        Assert.Equal(1u, Assert.Single(reader.Read(notRead)).Data!.index);
        Assert.False(notRead.TriggerValue);
        Assert.Throws<TimeoutException>(() => ws.Wait(TimeSpan.FromMilliseconds(50)));
        Assert.Single(ws.Conditions);
    }

    [Fact]
    public void TypeSupportAndClose()
    {
        Assert.Equal("HelloWorld::Greeting", TypeSupport.Get<Greeting>().TypeName);
        Assert.Equal(0u, TypeSupport.Get<Greeting>().NewValue().index);
        Assert.Throws<ArgumentException>(() => TypeSupport.Get<string>());
        Topic<Greeting> topic = dp.CreateTopic<Greeting>("supported");

        Publisher pub = dp.CreatePublisher();
        DataWriter<Greeting> writer = pub.CreateDataWriter(topic);
        writer.Dispose();
        Assert.Throws<AlreadyClosedException>(() => writer.Write(new Greeting(1, "x")));
        // Deleting a participant's entities closes them.
        Subscriber sub = dp.CreateSubscriber();
        DataReader<Greeting> reader = sub.CreateDataReader(topic);
        dp.DeleteContainedEntities();
        Assert.Throws<AlreadyClosedException>(() => reader.Take());
        Assert.Throws<AlreadyClosedException>(() => pub.CreateDataWriter(topic));
    }

    private static DomainParticipantQos SecureQos(string who)
    {
        string certs = "file:" + Path.Combine(TypesTests.Root(), "crates", "rustorb-dds-security", "tests", "certs") + "/";
        return DomainParticipantQos.Default with
        {
            Property = new Property(new Dictionary<string, string>
            {
                ["dds.sec.auth.identity_ca"] = certs + "identity_ca.pem",
                ["dds.sec.auth.identity_certificate"] = certs + who + "_cert.pem",
                ["dds.sec.auth.private_key"] = certs + who + "_key.pem",
                ["dds.sec.access.permissions_ca"] = certs + "permissions_ca.pem",
                ["dds.sec.access.governance"] = certs + "governance.p7s",
                ["dds.sec.access.permissions"] = certs + "permissions_" + who + ".p7s",
            }.ToImmutableDictionary()),
        };
    }

    [Fact]
    public void SecurityFromTheProperty()
    {
        int d = Interlocked.Increment(ref domain);
        // Some of the dds.sec properties only: refused.
        DomainParticipantQos partial = DomainParticipantQos.Default with
        {
            Property = new Property(ImmutableDictionary<string, string>.Empty.Add("dds.sec.auth.identity_ca", "file:/nowhere.pem")),
        };
        Assert.ThrowsAny<DdsException>(() => Participants.Localhost(d, partial));
        // A protected topic (Square* in the governance) between two
        // participants that authenticate each other.
        using DomainParticipant p1 = Participants.Localhost(d, SecureQos("p1"));
        using DomainParticipant p2 = Participants.Localhost(d, SecureQos("p2"));
        DataWriter<Greeting> writer = p1.CreatePublisher()
            .CreateDataWriter(p1.CreateTopic<Greeting>("Square_dotnet"), ReliableWriter());
        DataReader<Greeting> reader = p2.CreateSubscriber()
            .CreateDataReader(p2.CreateTopic<Greeting>("Square_dotnet"), ReliableReader());
        writer.WaitForSubscriptions(1, TenSeconds);
        writer.Write(new Greeting(1, "secret"));
        var got = new List<string>();
        Eventually(() =>
        {
            got.AddRange(reader.Take().Where(s => s.Info.ValidData).Select(s => s.Data!.message));
            return got.Count > 0;
        });
        Assert.Equal(new[] { "secret" }, got);
    }
}

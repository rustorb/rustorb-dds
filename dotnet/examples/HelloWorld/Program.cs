// Hello world over DDS, from .NET (rustorb-dds's Rustorb.Dds).
//
// In one terminal: HelloWorld sub
// In another:      HelloWorld pub
//
// The same as the Rust example hello_world (topic "hello_world" of
// HelloWorld::Greeting) and the same command line, used by interop/run.sh:
//
//   HelloWorld pub <domain> <count> <interval_ms> [best_effort|reliable|transient_local]
//   HelloWorld sub <domain> <count> <timeout_s> [best_effort|reliable|transient_local]
//
// The subscriber exits once it has count samples (all of them, in order,
// when reliable), with an error on timeout. With RUSTORB_DDS_LOCALHOST set,
// only the loopback interface is used.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Threading;
using HelloWorld;
using Omg.Dds.Core;
using Omg.Dds.Core.Policy;
using Omg.Dds.Core.Status;
using Omg.Dds.Domain;
using Omg.Dds.Pub;
using Omg.Dds.Sub;
using Omg.Dds.Topic;
using Rustorb.Dds;

internal static class Program
{
    private const string Message = "Hello, world!";

    internal static DomainParticipant Participant(int domain) =>
        Environment.GetEnvironmentVariable("RUSTORB_DDS_LOCALHOST") != null
            ? Participants.Localhost(domain)
            : new DomainParticipant(domain);

    internal static int Fail(string why)
    {
        Console.Error.WriteLine("rustorb-dds (.NET): " + why);
        return 1;
    }

    internal static void WaitForSubscriber(IDataWriter writer)
    {
        while (true)
        {
            try
            {
                writer.WaitForSubscriptions(1, TimeSpan.FromSeconds(1));
                return;
            }
            catch (TimeoutException)
            {
                // keep waiting
            }
        }
    }

    private static int Main(string[] args)
    {
        string role = args.Length > 0 ? args[0] : "pub";
        int domain = args.Length > 1 ? int.Parse(args[1]) : 0;
        int count = args.Length > 2 ? int.Parse(args[2]) : 10;
        string kind = args.Length > 4 ? args[4] : "best_effort";
        if (kind != "best_effort" && kind != "reliable" && kind != "transient_local")
        {
            return Fail("unknown reliability " + kind);
        }
        bool reliable = kind != "best_effort";
        bool transientLocal = kind == "transient_local";
        Durability durability = transientLocal ? Durability.TransientLocal() : Durability.Volatile();

        using DomainParticipant dp = Participant(domain);
        Topic<Greeting> topic = dp.CreateTopic<Greeting>("hello_world");
        if (role == "pub")
        {
            int interval = args.Length > 3 ? int.Parse(args[3]) : 1000;
            DataWriterQos qos = reliable
                ? DataWriterQos.Default with { Reliability = Reliability.Reliable(), History = History.KeepAll(), Durability = durability }
                : DataWriterQos.Default with { Reliability = Reliability.BestEffort() };
            DataWriter<Greeting> writer = dp.CreatePublisher().CreateDataWriter(topic, qos);
            if (!transientLocal)
            {
                Console.WriteLine("rustorb-dds (.NET): waiting for a subscriber...");
                WaitForSubscriber(writer);
                Thread.Sleep(500);
            }
            for (uint i = 1; i <= count; i++)
            {
                writer.Write(new Greeting(i, Message));
                Console.WriteLine("rustorb-dds (.NET): sent " + i);
                Thread.Sleep(interval);
            }
            if (transientLocal)
            {
                WaitForSubscriber(writer);
            }
            if (reliable)
            {
                writer.WaitForAcknowledgments(TimeSpan.FromSeconds(30));
                Console.WriteLine("rustorb-dds (.NET): all acknowledged");
            }
        }
        else if (role == "sub")
        {
            int timeout = args.Length > 3 ? int.Parse(args[3]) : 3600;
            DateTime deadline = DateTime.UtcNow.AddSeconds(timeout);
            DataReaderQos qos = DataReaderQos.Default with
            {
                Reliability = reliable ? Reliability.Reliable() : Reliability.BestEffort(),
                History = History.KeepAll(),
                Durability = durability,
            };
            if (transientLocal)
            {
                Thread.Sleep(2000);
            }
            DataReader<Greeting> reader = dp.CreateSubscriber().CreateDataReader(topic, qos);
            StatusCondition status = reader.StatusCondition;
            status.EnabledStatuses = StatusKind.DataAvailable;
            using var waitset = new WaitSet();
            waitset.AttachCondition(status);
            Console.WriteLine("rustorb-dds (.NET): waiting for greetings...");
            int got = 0;
            while (got < count)
            {
                TimeSpan left = deadline - DateTime.UtcNow;
                try
                {
                    waitset.Wait(left > TimeSpan.Zero ? left : TimeSpan.Zero);
                }
                catch (TimeoutException)
                {
                    return Fail($"got {got} of {count} samples");
                }
                foreach (Sample<Greeting> s in reader.Take())
                {
                    if (!s.Info.ValidData || s.Data is not Greeting g)
                    {
                        continue;
                    }
                    if (g.message != Message)
                    {
                        return Fail($"bad message {g.message} for {g.index}");
                    }
                    Console.WriteLine("rustorb-dds (.NET): received " + g.index);
                    got++;
                    if (reliable && g.index != got)
                    {
                        return Fail($"expected {got}, got {g.index}");
                    }
                }
            }
        }
        else
        {
            return Fail($"unknown role {role}: use pub or sub");
        }
        return 0;
    }
}

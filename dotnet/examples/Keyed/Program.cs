// Keyed instances over DDS, from .NET: write, dispose and unregister.
//
// The same as the Rust example keyed (topic "keyed" of Keyed::Shape, keyed
// by id) and the same command line, used by interop/run.sh:
//
//   Keyed pub <domain> <count> <interval_ms>
//   Keyed sub <domain> <count> <timeout_s>
//
// The publisher writes count samples to the instances id 1, 2, 3 in turn,
// then disposes of id 2 and unregisters id 3. The subscriber exits once it
// has count samples, each instance's in order, and has seen id 2
// NOT_ALIVE_DISPOSED and id 3 NOT_ALIVE_NO_WRITERS. With
// RUSTORB_DDS_LOCALHOST set, only the loopback interface is used.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections.Generic;
using System.Threading;
using Keyed;
using Omg.Dds.Core;
using Omg.Dds.Core.Policy;
using Omg.Dds.Domain;
using Omg.Dds.Pub;
using Omg.Dds.Sub;
using Omg.Dds.Topic;
using Rustorb.Dds;

internal static class Program
{
    private const string Message = "Hello, instance!";

    private static int Fail(string why)
    {
        Console.Error.WriteLine("rustorb-dds (.NET): " + why);
        return 1;
    }

    private static int Main(string[] args)
    {
        string role = args.Length > 0 ? args[0] : "pub";
        int domain = args.Length > 1 ? int.Parse(args[1]) : 0;
        int count = args.Length > 2 ? int.Parse(args[2]) : 30;
        using DomainParticipant dp = Environment.GetEnvironmentVariable("RUSTORB_DDS_LOCALHOST") != null
            ? Participants.Localhost(domain)
            : new DomainParticipant(domain);
        Topic<Shape> topic = dp.CreateTopic<Shape>("keyed");
        if (role == "pub")
        {
            int interval = args.Length > 3 ? int.Parse(args[3]) : 100;
            DataWriter<Shape> writer = dp.CreatePublisher().CreateDataWriter(topic, DataWriterQos.Default with
            {
                Reliability = Reliability.Reliable(),
                History = History.KeepAll(),
                WriterDataLifecycle = new WriterDataLifecycle(false),
            });
            Console.WriteLine("rustorb-dds (.NET): waiting for a subscriber...");
            while (true)
            {
                try
                {
                    writer.WaitForSubscriptions(1, TimeSpan.FromSeconds(1));
                    break;
                }
                catch (TimeoutException)
                {
                    // keep waiting
                }
            }
            Thread.Sleep(500);
            writer.RegisterInstance(new Shape(2, 0, ""));
            for (uint i = 1; i <= count; i++)
            {
                uint id = (i - 1) % 3 + 1;
                writer.Write(new Shape(id, i, Message));
                Console.WriteLine($"rustorb-dds (.NET): sent id {id} index {i}");
                Thread.Sleep(interval);
            }
            writer.DisposeInstance(new Shape(2, 0, ""));
            writer.UnregisterInstance(new Shape(3, 0, ""));
            Console.WriteLine("rustorb-dds (.NET): disposed 2, unregistered 3");
            writer.WaitForAcknowledgments(TimeSpan.FromSeconds(30));
            Console.WriteLine("rustorb-dds (.NET): all acknowledged");
        }
        else if (role == "sub")
        {
            int timeout = args.Length > 3 ? int.Parse(args[3]) : 3600;
            DateTime deadline = DateTime.UtcNow.AddSeconds(timeout);
            DataReader<Shape> reader = dp.CreateSubscriber().CreateDataReader(topic, DataReaderQos.Default with
            {
                Reliability = Reliability.Reliable(),
                History = History.KeepAll(),
            });
            Console.WriteLine("rustorb-dds (.NET): waiting for shapes...");
            int got = 0;
            var last = new Dictionary<uint, uint>();
            // The id of each instance, from its samples: a sample without
            // data can come after its instance is gone.
            var ids = new Dictionary<InstanceHandle, uint>();
            bool disposed = false;
            bool noWriters = false;
            while (got < count || !disposed || !noWriters)
            {
                if (DateTime.UtcNow >= deadline)
                {
                    return Fail($"got {got} of {count} samples, disposed {disposed}, no writers {noWriters}");
                }
                try
                {
                    reader.WaitForData(TimeSpan.FromMilliseconds(200));
                }
                catch (TimeoutException)
                {
                    continue;
                }
                foreach (Sample<Shape> s in reader.Take())
                {
                    if (s.Info.ValidData && s.Data is Shape d)
                    {
                        last.TryGetValue(d.id, out uint previous);
                        if (d.message != Message || d.index <= previous || (d.index - 1) % 3 + 1 != d.id)
                        {
                            return Fail($"bad sample id {d.id} index {d.index}");
                        }
                        last[d.id] = d.index;
                        ids[s.Info.InstanceHandle] = d.id;
                        got++;
                        Console.WriteLine($"rustorb-dds (.NET): received id {d.id} index {d.index}");
                    }
                    else
                    {
                        if (!ids.TryGetValue(s.Info.InstanceHandle, out uint id))
                        {
                            id = reader.GetKeyValue(s.Info.InstanceHandle).id;
                        }
                        InstanceState state = s.Info.InstanceState;
                        Console.WriteLine($"rustorb-dds (.NET): id {id} is {state}");
                        disposed |= id == 2 && state == InstanceState.NotAliveDisposed;
                        noWriters |= id == 3 && state == InstanceState.NotAliveNoWriters;
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

// ping of bench/bindings.py in C# (rustorb-dds#107): the round trip
// through `rustorb-dds-perf pong`, as `rustorb-dds-perf ping` measures it.
//
//   Ping <domain> <sizes> <warmup_s> <duration_s>
//
// Writes RustorbPerf.Sample on RustorbPerf_ping (reliable, KEEP_LAST 1),
// waits on a WaitSet for the sample of the same seq on RustorbPerf_pong,
// and prints one line of JSON per size.

using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using Omg.Dds.Core;
using Omg.Dds.Core.Policy;
using Omg.Dds.Core.Status;
using Omg.Dds.Domain;
using Omg.Dds.Pub;
using Omg.Dds.Sub;
using Omg.Dds.Topic;
using PerfSample = RustorbPerf.Sample;

internal static class Program
{
    private static double At(List<long> ticks, double p)
    {
        int n = ticks.Count;
        if (n == 0)
        {
            return 0;
        }
        int i = Math.Min(Math.Max((int)Math.Ceiling(p * n), 1), n) - 1;
        return Us(ticks[i]);
    }

    private static double Us(long ticks) => ticks * 1e6 / Stopwatch.Frequency;

    private static int Main(string[] args)
    {
        int domain = int.Parse(args[0], CultureInfo.InvariantCulture);
        string[] sizes = args[1].Split(',');
        long warmup = (long)(double.Parse(args[2], CultureInfo.InvariantCulture) * Stopwatch.Frequency);
        long duration = (long)(double.Parse(args[3], CultureInfo.InvariantCulture) * Stopwatch.Frequency);
        using DomainParticipant dp = new DomainParticipant(domain);
        Topic<PerfSample> pingTopic = dp.CreateTopic<PerfSample>("RustorbPerf_ping");
        Topic<PerfSample> pongTopic = dp.CreateTopic<PerfSample>("RustorbPerf_pong");
        DataWriter<PerfSample> writer = dp.CreatePublisher().CreateDataWriter(pingTopic,
            DataWriterQos.Default with { Reliability = Reliability.Reliable(), History = History.KeepLast(1) });
        DataReader<PerfSample> reader = dp.CreateSubscriber().CreateDataReader(pongTopic,
            DataReaderQos.Default with { Reliability = Reliability.Reliable(), History = History.KeepLast(1) });
        writer.WaitForSubscriptions(1, TimeSpan.FromSeconds(30));
        StatusCondition status = reader.StatusCondition;
        status.EnabledStatuses = StatusKind.DataAvailable;
        using var waitset = new WaitSet();
        waitset.AttachCondition(status);
        long start = Stopwatch.GetTimestamp();
        ulong seq = 0;
        foreach (string sizeText in sizes)
        {
            int size = int.Parse(sizeText, CultureInfo.InvariantCulture);
            var sample = new PerfSample(0, 0, new Omg.Types.Sequence<byte>(new byte[size]));
            reader.Take();
            long warmEnd = Stopwatch.GetTimestamp() + warmup;
            long end = warmEnd + duration;
            var rtts = new List<long>();
            long lost = 0;
            while (Stopwatch.GetTimestamp() < end)
            {
                bool measuring = Stopwatch.GetTimestamp() >= warmEnd;
                seq++;
                long sent = Stopwatch.GetTimestamp();
                sample.seq = seq;
                sample.stamp = (ulong)(sent - start);
                writer.Write(sample);
                long deadline = sent + Stopwatch.Frequency;
                while (true)
                {
                    long left = deadline - Stopwatch.GetTimestamp();
                    try
                    {
                        waitset.Wait(TimeSpan.FromSeconds(Math.Max(0, (double)left / Stopwatch.Frequency)));
                    }
                    catch (TimeoutException)
                    {
                        if (measuring)
                        {
                            lost++;
                        }
                        break;
                    }
                    bool got = false;
                    foreach (Sample<PerfSample> s in reader.Take())
                    {
                        if (s.Info.ValidData && s.Data is PerfSample d && d.seq == seq)
                        {
                            got = true;
                        }
                    }
                    if (got)
                    {
                        if (measuring)
                        {
                            rtts.Add(Stopwatch.GetTimestamp() - sent);
                        }
                        break;
                    }
                }
            }
            rtts.Sort();
            Console.WriteLine(string.Format(CultureInfo.InvariantCulture,
                "{{\"tool\":\"bench/bindings/dotnet\",\"impl\":\"rustorb-dds-dotnet\",\"mode\":\"ping\",\"size\":{0},"
                + "\"reliability\":\"reliable\",\"lost\":{1},\"seconds\":{2:F3},\"count\":{3},\"rtt_min_us\":{4:F3},"
                + "\"rtt_p50_us\":{5:F3},\"rtt_p90_us\":{6:F3},\"rtt_p99_us\":{7:F3},\"rtt_max_us\":{8:F3}}}",
                size, lost, (double)duration / Stopwatch.Frequency, rtts.Count, At(rtts, 0), At(rtts, 0.5),
                At(rtts, 0.9), At(rtts, 0.99), rtts.Count == 0 ? 0.0 : Us(rtts[^1])));
        }
        return 0;
    }
}

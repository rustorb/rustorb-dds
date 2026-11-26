// ping of bench/bindings.py in Java (rustorb-dds#107): the round trip
// through `rustorb-dds-perf pong`, as `rustorb-dds-perf ping` measures it.
//
//   java ... Ping <domain> <sizes> <warmup_s> <duration_s>
//
// Writes RustorbPerf.Sample (bench/bindings/generated/java, from
// rustorb-idl -bjava -Wbdds_only) on RustorbPerf_ping (reliable,
// KEEP_LAST 1), waits on a WaitSet for the sample of the same seq on
// RustorbPerf_pong, and prints one line of JSON per size.

import RustorbPerf.Sample;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.WaitSet;
import org.omg.dds.core.policy.PolicyFactory;
import org.omg.dds.core.status.DataAvailableStatus;
import org.omg.dds.domain.DomainParticipant;
import org.omg.dds.pub.DataWriter;
import org.omg.dds.pub.Publisher;
import org.omg.dds.sub.DataReader;
import org.omg.dds.sub.Subscriber;
import org.omg.dds.topic.Topic;
import org.rustorb.dds.Rustorb;

public final class Ping {
    private Ping() {}

    static double at(List<Long> ns, double p) {
        int n = ns.size();
        if (n == 0) {
            return 0;
        }
        int i = Math.min(Math.max((int) Math.ceil(p * n), 1), n) - 1;
        return ns.get(i) / 1000.0;
    }

    public static void main(String[] args) throws Exception {
        int domain = Integer.parseInt(args[0]);
        String[] sizes = args[1].split(",");
        long warmup = (long) (Double.parseDouble(args[2]) * 1e9);
        long duration = (long) (Double.parseDouble(args[3]) * 1e9);
        ServiceEnvironment env = ServiceEnvironment.createInstance(Ping.class.getClassLoader());
        PolicyFactory policy = PolicyFactory.getPolicyFactory(env);
        DomainParticipant dp = env.getSPI().getParticipantFactory().createParticipant(domain);
        try {
            Topic<Sample> pingTopic = dp.createTopic("RustorbPerf_ping", Sample.class);
            Topic<Sample> pongTopic = dp.createTopic("RustorbPerf_pong", Sample.class);
            Publisher pub = dp.createPublisher();
            Subscriber sub = dp.createSubscriber();
            DataWriter<Sample> writer = pub.createDataWriter(pingTopic, pub.getDefaultDataWriterQos()
                    .withPolicies(policy.Reliability().withReliable(), policy.History().withKeepLast(1)));
            DataReader<Sample> reader = sub.createDataReader(pongTopic, sub.getDefaultDataReaderQos()
                    .withPolicies(policy.Reliability().withReliable(), policy.History().withKeepLast(1)));
            Rustorb.waitForSubscriptions(writer, 1, org.omg.dds.core.Duration.newDuration(30, TimeUnit.SECONDS, env));
            StatusCondition<DataReader<Sample>> status = reader.getStatusCondition();
            status.setEnabledStatuses(List.of(DataAvailableStatus.class));
            WaitSet waitset = WaitSet.newWaitSet(env);
            waitset.attachCondition(status);
            long start = System.nanoTime();
            long seq = 0;
            for (String sizeText : sizes) {
                int size = Integer.parseInt(sizeText);
                Sample sample = new Sample(0, 0, new byte[size]);
                reader.take().close();
                long warmEnd = System.nanoTime() + warmup;
                long end = warmEnd + duration;
                List<Long> rtts = new ArrayList<>();
                long lost = 0;
                while (System.nanoTime() < end) {
                    boolean measuring = System.nanoTime() >= warmEnd;
                    seq++;
                    long sent = System.nanoTime();
                    sample.seq = seq;
                    sample.stamp = sent - start;
                    writer.write(sample);
                    long deadline = sent + 1_000_000_000L;
                    while (true) {
                        try {
                            waitset.waitForConditions(Math.max(0, deadline - System.nanoTime()), TimeUnit.NANOSECONDS);
                        } catch (TimeoutException e) {
                            if (measuring) {
                                lost++;
                            }
                            break;
                        }
                        boolean got = false;
                        try (org.omg.dds.sub.Sample.Iterator<Sample> it = reader.take()) {
                            while (it.hasNext()) {
                                Sample s = it.next().getData();
                                if (s != null && s.seq == seq) {
                                    got = true;
                                }
                            }
                        }
                        if (got) {
                            if (measuring) {
                                rtts.add(System.nanoTime() - sent);
                            }
                            break;
                        }
                    }
                }
                Collections.sort(rtts);
                System.out.println(String.format(Locale.ROOT,
                        "{\"tool\":\"bench/bindings/java\",\"impl\":\"rustorb-dds-java\",\"mode\":\"ping\",\"size\":%d,"
                                + "\"reliability\":\"reliable\",\"lost\":%d,\"seconds\":%.3f,\"count\":%d,"
                                + "\"rtt_min_us\":%.3f,\"rtt_p50_us\":%.3f,\"rtt_p90_us\":%.3f,\"rtt_p99_us\":%.3f,"
                                + "\"rtt_max_us\":%.3f}",
                        size, lost, duration / 1e9, rtts.size(), at(rtts, 0), at(rtts, 0.5), at(rtts, 0.9),
                        at(rtts, 0.99), rtts.isEmpty() ? 0.0 : rtts.get(rtts.size() - 1) / 1000.0));
            }
        } finally {
            dp.close();
        }
        System.exit(0);
    }
}

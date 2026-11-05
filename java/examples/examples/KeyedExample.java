// Keyed instances over DDS, from Java: write, dispose and unregister.
//
// The same as the Rust example keyed (topic "keyed" of Keyed::Shape, keyed
// by id) and the same command line, used by interop/run.sh:
//
//   KeyedExample pub <domain> <count> <interval_ms>
//   KeyedExample sub <domain> <count> <timeout_s>
//
// The publisher writes count samples to the instances id 1, 2, 3 in turn,
// then disposes of id 2 and unregisters id 3. The subscriber exits once it
// has count samples, each instance's in order, and has seen id 2
// NOT_ALIVE_DISPOSED and id 3 NOT_ALIVE_NO_WRITERS. With
// RUSTORB_DDS_LOCALHOST set, only the loopback interface is used.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package examples;

import Keyed.Shape;
import java.util.HashMap;
import java.util.Map;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import org.omg.dds.core.Duration;
import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.policy.PolicyFactory;
import org.omg.dds.domain.DomainParticipant;
import org.omg.dds.pub.DataWriter;
import org.omg.dds.pub.Publisher;
import org.omg.dds.sub.DataReader;
import org.omg.dds.sub.InstanceState;
import org.omg.dds.sub.Sample;
import org.omg.dds.sub.Subscriber;
import org.omg.dds.topic.Topic;
import org.rustorb.dds.Rustorb;

public final class KeyedExample {
    private static final String MESSAGE = "Hello, instance!";

    private KeyedExample() {}

    public static void main(String[] args) throws Exception {
        System.exit(run(args));
    }

    static int run(String[] args) throws Exception {
        String role = args.length > 0 ? args[0] : "pub";
        int domain = args.length > 1 ? Integer.parseInt(args[1]) : 0;
        int count = args.length > 2 ? Integer.parseInt(args[2]) : 30;
        ServiceEnvironment env = ServiceEnvironment.createInstance(KeyedExample.class.getClassLoader());
        PolicyFactory policy = PolicyFactory.getPolicyFactory(env);
        DomainParticipant dp = HelloWorldExample.participant(env, domain);
        try {
            Topic<Shape> topic = dp.createTopic("keyed", Shape.class);
            if (role.equals("pub")) {
                int interval = args.length > 3 ? Integer.parseInt(args[3]) : 100;
                Publisher pub = dp.createPublisher();
                DataWriter<Shape> writer = pub.createDataWriter(topic, pub.getDefaultDataWriterQos().withPolicies(
                        policy.Reliability().withReliable(), policy.History().withKeepAll(),
                        policy.WriterDataLifecycle().withAutDisposeUnregisteredInstances(false)));
                System.out.println("rustorb-dds (Java): waiting for a subscriber...");
                HelloWorldExample.waitForSubscriber(writer, Duration.newDuration(1, TimeUnit.SECONDS, env));
                Thread.sleep(500);
                InstanceHandle two = writer.registerInstance(new Shape(2, 0, ""));
                for (int i = 1; i <= count; i++) {
                    int id = (i - 1) % 3 + 1;
                    writer.write(new Shape(id, i, MESSAGE));
                    System.out.println("rustorb-dds (Java): sent id " + id + " index " + i);
                    Thread.sleep(interval);
                }
                writer.dispose(two);
                writer.unregisterInstance(writer.lookupInstance(new Shape(3, 0, "")));
                System.out.println("rustorb-dds (Java): disposed 2, unregistered 3");
                writer.waitForAcknowledgments(30, TimeUnit.SECONDS);
                System.out.println("rustorb-dds (Java): all acknowledged");
            } else if (role.equals("sub")) {
                int timeout = args.length > 3 ? Integer.parseInt(args[3]) : 3600;
                long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(timeout);
                Subscriber sub = dp.createSubscriber();
                DataReader<Shape> reader = sub.createDataReader(topic, sub.getDefaultDataReaderQos().withPolicies(
                        policy.Reliability().withReliable(), policy.History().withKeepAll()));
                System.out.println("rustorb-dds (Java): waiting for shapes...");
                Duration step = Duration.newDuration(200, TimeUnit.MILLISECONDS, env);
                int got = 0;
                Map<Integer, Integer> last = new HashMap<>();
                // The id of each instance, from its samples (an invalid
                // sample has no data in the Java PSM).
                Map<InstanceHandle, Integer> ids = new HashMap<>();
                boolean disposed = false;
                boolean noWriters = false;
                while (got < count || !disposed || !noWriters) {
                    if (System.nanoTime() >= deadline) {
                        return HelloWorldExample.fail("got " + got + " of " + count + " samples, disposed "
                                + disposed + ", no writers " + noWriters);
                    }
                    try {
                        Rustorb.waitForData(reader, step);
                    } catch (TimeoutException e) {
                        continue;
                    }
                    try (Sample.Iterator<Shape> it = reader.take()) {
                        while (it.hasNext()) {
                            Sample<Shape> s = it.next();
                            Shape d = s.getData();
                            if (d != null) {
                                int previous = last.getOrDefault(d.id, 0);
                                if (!d.message.equals(MESSAGE) || d.index <= previous || (d.index - 1) % 3 + 1 != d.id) {
                                    return HelloWorldExample.fail("bad sample id " + d.id + " index " + d.index);
                                }
                                last.put(d.id, d.index);
                                ids.put(s.getInstanceHandle().immutableCopy(), d.id);
                                got++;
                                System.out.println("rustorb-dds (Java): received id " + d.id + " index " + d.index);
                            } else {
                                Integer id = ids.get(s.getInstanceHandle().immutableCopy());
                                if (id == null) {
                                    id = reader.getKeyValue(null, s.getInstanceHandle()).id;
                                }
                                InstanceState state = s.getInstanceState();
                                System.out.println("rustorb-dds (Java): id " + id + " is " + state);
                                disposed |= id == 2 && state == InstanceState.NOT_ALIVE_DISPOSED;
                                noWriters |= id == 3 && state == InstanceState.NOT_ALIVE_NO_WRITERS;
                            }
                        }
                    }
                }
            } else {
                return HelloWorldExample.fail("unknown role " + role + ": use pub or sub");
            }
        } finally {
            dp.close();
        }
        return 0;
    }
}

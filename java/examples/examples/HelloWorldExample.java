// Hello world over DDS, from Java (the DDS Java 5 PSM of rustorb-dds).
//
// In one terminal: HelloWorldExample sub
// In another:      HelloWorldExample pub
//
// The same as the Rust example hello_world (topic "hello_world" of
// HelloWorld::Greeting) and the same command line, used by interop/run.sh:
//
//   HelloWorldExample pub <domain> <count> <interval_ms> [best_effort|reliable|transient_local]
//   HelloWorldExample sub <domain> <count> <timeout_s> [best_effort|reliable|transient_local]
//
// The subscriber exits once it has count samples (all of them, in order,
// when reliable), with an error on timeout. With RUSTORB_DDS_LOCALHOST set,
// only the loopback interface is used.
//
// HelloWorld.Greeting and HelloWorld.GreetingCodec are what
// `rustorb-idl -bjava -Wbdds_only` writes for HelloWorld.idl.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package examples;

import HelloWorld.Greeting;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import org.omg.dds.core.Duration;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.WaitSet;
import org.omg.dds.core.policy.PolicyFactory;
import org.omg.dds.core.status.DataAvailableStatus;
import org.omg.dds.domain.DomainParticipant;
import org.omg.dds.pub.DataWriter;
import org.omg.dds.pub.DataWriterQos;
import org.omg.dds.pub.Publisher;
import org.omg.dds.sub.DataReader;
import org.omg.dds.sub.DataReaderQos;
import org.omg.dds.sub.Sample;
import org.omg.dds.sub.Subscriber;
import org.omg.dds.topic.Topic;
import org.rustorb.dds.Rustorb;

public final class HelloWorldExample {
    private static final String MESSAGE = "Hello, world!";

    private HelloWorldExample() {}

    static DomainParticipant participant(ServiceEnvironment env, int domain) {
        if (System.getenv("RUSTORB_DDS_LOCALHOST") != null) {
            return Rustorb.localhostParticipant(env, domain);
        }
        return env.getSPI().getParticipantFactory().createParticipant(domain);
    }

    static int fail(String why) {
        System.err.println("rustorb-dds (Java): " + why);
        return 1;
    }

    public static void main(String[] args) throws Exception {
        System.exit(run(args));
    }

    static int run(String[] args) throws Exception {
        String role = args.length > 0 ? args[0] : "pub";
        int domain = args.length > 1 ? Integer.parseInt(args[1]) : 0;
        int count = args.length > 2 ? Integer.parseInt(args[2]) : 10;
        String kind = args.length > 4 ? args[4] : "best_effort";
        if (!kind.equals("best_effort") && !kind.equals("reliable") && !kind.equals("transient_local")) {
            return fail("unknown reliability " + kind);
        }
        boolean reliable = !kind.equals("best_effort");
        boolean transientLocal = kind.equals("transient_local");

        ServiceEnvironment env = ServiceEnvironment.createInstance(HelloWorldExample.class.getClassLoader());
        PolicyFactory policy = PolicyFactory.getPolicyFactory(env);
        DomainParticipant dp = participant(env, domain);
        try {
            Topic<Greeting> topic = dp.createTopic("hello_world", Greeting.class);
            if (role.equals("pub")) {
                int interval = args.length > 3 ? Integer.parseInt(args[3]) : 1000;
                Publisher pub = dp.createPublisher();
                DataWriterQos qos = pub.getDefaultDataWriterQos();
                if (reliable) {
                    qos = qos.withPolicies(policy.Reliability().withReliable(), policy.History().withKeepAll(),
                            transientLocal ? policy.Durability().withTransientLocal()
                                    : policy.Durability().withVolatile());
                } else {
                    qos = qos.withPolicy(policy.Reliability().withBestEffort());
                }
                DataWriter<Greeting> writer = pub.createDataWriter(topic, qos);
                Duration second = Duration.newDuration(1, TimeUnit.SECONDS, env);
                if (!transientLocal) {
                    System.out.println("rustorb-dds (Java): waiting for a subscriber...");
                    waitForSubscriber(writer, second);
                    Thread.sleep(500);
                }
                for (int i = 1; i <= count; i++) {
                    writer.write(new Greeting(i, MESSAGE));
                    System.out.println("rustorb-dds (Java): sent " + i);
                    Thread.sleep(interval);
                }
                if (transientLocal) {
                    waitForSubscriber(writer, second);
                }
                if (reliable) {
                    writer.waitForAcknowledgments(30, TimeUnit.SECONDS);
                    System.out.println("rustorb-dds (Java): all acknowledged");
                }
            } else if (role.equals("sub")) {
                int timeout = args.length > 3 ? Integer.parseInt(args[3]) : 3600;
                long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(timeout);
                Subscriber sub = dp.createSubscriber();
                DataReaderQos qos = sub.getDefaultDataReaderQos().withPolicies(
                        reliable ? policy.Reliability().withReliable() : policy.Reliability().withBestEffort(),
                        policy.History().withKeepAll(),
                        transientLocal ? policy.Durability().withTransientLocal()
                                : policy.Durability().withVolatile());
                if (transientLocal) {
                    Thread.sleep(2000);
                }
                DataReader<Greeting> reader = sub.createDataReader(topic, qos);
                StatusCondition<DataReader<Greeting>> status = reader.getStatusCondition();
                status.setEnabledStatuses(java.util.List.of(DataAvailableStatus.class));
                WaitSet waitset = WaitSet.newWaitSet(env);
                waitset.attachCondition(status);
                System.out.println("rustorb-dds (Java): waiting for greetings...");
                int got = 0;
                while (got < count) {
                    long left = Math.max(0, deadline - System.nanoTime());
                    try {
                        waitset.waitForConditions(left, TimeUnit.NANOSECONDS);
                    } catch (TimeoutException e) {
                        return fail("got " + got + " of " + count + " samples");
                    }
                    try (Sample.Iterator<Greeting> it = reader.take()) {
                        while (it.hasNext()) {
                            Sample<Greeting> s = it.next();
                            Greeting g = s.getData();
                            if (g == null) {
                                continue;
                            }
                            if (!g.message.equals(MESSAGE)) {
                                return fail("bad message " + g.message + " for " + g.index);
                            }
                            System.out.println("rustorb-dds (Java): received " + g.index);
                            got++;
                            if (reliable && g.index != got) {
                                return fail("expected " + got + ", got " + g.index);
                            }
                        }
                    }
                }
            } else {
                return fail("unknown role " + role + ": use pub or sub");
            }
        } finally {
            dp.close();
        }
        return 0;
    }

    static void waitForSubscriber(DataWriter<?> writer, Duration step) {
        while (true) {
            try {
                Rustorb.waitForSubscriptions(writer, 1, step);
                return;
            } catch (TimeoutException e) {
                // keep waiting
            }
        }
    }
}

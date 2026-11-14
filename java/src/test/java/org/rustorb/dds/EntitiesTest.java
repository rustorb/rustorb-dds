// The entities of the Java binding on a loopback-only domain: writers and
// readers, keyed instances, listeners, conditions and wait sets, QoS and
// statuses, and what this subset of the PSM does not do.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import HelloWorld.Greeting;
import Keyed.Shape;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.omg.dds.core.AlreadyClosedException;
import org.omg.dds.core.Condition;
import org.omg.dds.core.Duration;
import org.omg.dds.core.GuardCondition;
import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.WaitSet;
import org.omg.dds.core.event.DataAvailableEvent;
import org.omg.dds.core.event.DataOnReadersEvent;
import org.omg.dds.core.event.PublicationMatchedEvent;
import org.omg.dds.core.event.SubscriptionMatchedEvent;
import org.omg.dds.core.policy.PolicyFactory;
import org.omg.dds.core.policy.Reliability;
import org.omg.dds.core.status.DataAvailableStatus;
import org.omg.dds.core.status.DataOnReadersStatus;
import org.omg.dds.core.status.PublicationMatchedStatus;
import org.omg.dds.core.status.Status;
import org.omg.dds.core.status.SubscriptionMatchedStatus;
import org.omg.dds.domain.DomainParticipant;
import org.omg.dds.domain.DomainParticipantAdapter;
import org.omg.dds.pub.DataWriter;
import org.omg.dds.pub.DataWriterAdapter;
import org.omg.dds.pub.DataWriterQos;
import org.omg.dds.pub.Publisher;
import org.omg.dds.sub.DataReader;
import org.omg.dds.sub.DataReaderAdapter;
import org.omg.dds.sub.DataReaderQos;
import org.omg.dds.sub.InstanceState;
import org.omg.dds.sub.ReadCondition;
import org.omg.dds.sub.Sample;
import org.omg.dds.sub.SampleState;
import org.omg.dds.sub.Subscriber;
import org.omg.dds.sub.SubscriberAdapter;
import org.omg.dds.sub.ViewState;
import org.omg.dds.topic.ContentFilteredTopic;
import org.omg.dds.topic.Topic;
import org.omg.dds.type.TypeSupport;

class EntitiesTest {
    // Domains of their own, so that other tests running at the same time
    // do not match (ports from 49152 up can be reserved on Windows).
    private static final AtomicInteger DOMAIN = new AtomicInteger(40);

    private ServiceEnvironment env;
    private PolicyFactory policy;
    private DomainParticipant dp;
    private Duration tenSeconds;

    @BeforeEach
    void setUp() {
        env = ServiceEnvironment.createInstance(EntitiesTest.class.getClassLoader());
        policy = PolicyFactory.getPolicyFactory(env);
        dp = Rustorb.localhostParticipant(env, DOMAIN.getAndIncrement());
        tenSeconds = Duration.newDuration(10, TimeUnit.SECONDS, env);
    }

    @AfterEach
    void tearDown() {
        dp.close();
    }

    private DataWriterQos reliableWriter(Publisher pub) {
        return pub.getDefaultDataWriterQos().withPolicies(policy.Reliability().withReliable(),
                policy.History().withKeepAll());
    }

    private DataReaderQos reliableReader(Subscriber sub) {
        return sub.getDefaultDataReaderQos().withPolicies(policy.Reliability().withReliable(),
                policy.History().withKeepAll());
    }

    private static <T> List<Sample<T>> takeAll(DataReader<T> reader) {
        List<Sample<T>> out = new ArrayList<>();
        reader.take(out);
        return out;
    }

    @Test
    void environmentAndParticipant() {
        assertTrue(env instanceof RustorbServiceEnvironment);
        assertEquals(DOMAIN.get() - 1, dp.getDomainId());
        assertTrue(dp.getCurrentTime().isValid());
        assertFalse(dp.getInstanceHandle().isNil());
        assertTrue(Rustorb.discoveredParticipants(dp) >= 0);
        dp.assertLiveliness();
        assertTrue(env.getSPI().nilHandle().isNil());
        assertTrue(env.getSPI().infiniteDuration().isInfinite());
        assertEquals(1500, Duration.newDuration(1500, TimeUnit.MILLISECONDS, env).getDuration(TimeUnit.MILLISECONDS));
    }

    @Test
    void groupListeners() throws Exception {
        CountDownLatch published = new CountDownLatch(1);
        List<String> calls = new CopyOnWriteArrayList<>();
        dp.setListener(new DomainParticipantAdapter() {
            @Override
            public void onPublicationMatched(PublicationMatchedEvent<?> e) {
                calls.add("participant:matched " + e.getSource().getTopic().getName());
                published.countDown();
            }

            @Override
            public void onDataAvailable(DataAvailableEvent<?> e) {
                calls.add("participant:data_available");
            }
        }, List.of(PublicationMatchedStatus.class, DataAvailableStatus.class));
        assertNotNull(dp.getListener());
        Topic<Greeting> topic = dp.createTopic("grouped", Greeting.class);
        Publisher pub = dp.createPublisher();
        CountDownLatch onReaders = new CountDownLatch(1);
        CountDownLatch readerData = new CountDownLatch(1);
        Subscriber sub = dp.createSubscriber(dp.getDefaultSubscriberQos(), new SubscriberAdapter() {
            @Override
            public void onDataOnReaders(DataOnReadersEvent e) {
                calls.add("subscriber:data_on_readers");
                e.getSource().notifyDataReaders();
                onReaders.countDown();
            }
        }, List.of(DataOnReadersStatus.class));
        DataReader<Greeting> reader = sub.createDataReader(topic, reliableReader(sub),
                new DataReaderAdapter<Greeting>() {
                    @Override
                    public void onDataAvailable(DataAvailableEvent<Greeting> e) {
                        calls.add("reader:data_available");
                        readerData.countDown();
                    }
                }, List.of(DataAvailableStatus.class));
        DataWriter<Greeting> writer = pub.createDataWriter(topic, reliableWriter(pub));
        assertTrue(published.await(10, TimeUnit.SECONDS));
        assertTrue(calls.contains("participant:matched grouped"));
        writer.write(new Greeting(1, "g"));
        assertTrue(onReaders.await(10, TimeUnit.SECONDS));
        assertTrue(readerData.await(10, TimeUnit.SECONDS));
        assertFalse(calls.contains("participant:data_available"));
        assertEquals(1, takeAll(reader).size());
        dp.setListener(null);
        assertNull(dp.getListener());
    }

    @Test
    void contentFilteredTopic() throws Exception {
        Topic<Greeting> topic = dp.createTopic("filtered_greetings", Greeting.class);
        ContentFilteredTopic<Greeting> cft = dp.createContentFilteredTopic("big_greetings", topic,
                "index > %0 AND message LIKE 'h%'", "3");
        assertEquals("big_greetings", cft.getName());
        assertEquals("HelloWorld::Greeting", cft.getTypeName());
        assertSame(topic, cft.getRelatedTopic());
        assertSame(cft, dp.lookupTopicDescription("big_greetings"));
        assertEquals(List.of("3"), cft.getExpressionParameters());
        Publisher pub = dp.createPublisher();
        Subscriber sub = dp.createSubscriber();
        DataWriter<Greeting> writer = pub.createDataWriter(topic, reliableWriter(pub));
        DataReader<Greeting> reader = sub.createDataReader(cft, reliableReader(sub));
        assertSame(cft, reader.getTopicDescription());
        Rustorb.waitForSubscriptions(writer, 1, tenSeconds);
        for (int i = 1; i <= 6; i++) {
            writer.write(new Greeting(i, i == 5 ? "bye" : "hello"));
        }
        writer.waitForAcknowledgments(tenSeconds);
        List<Integer> got = new ArrayList<>();
        long end = System.nanoTime() + 10_000_000_000L;
        while (got.size() < 2 && System.nanoTime() < end) {
            for (Sample<Greeting> s : takeAll(reader)) {
                got.add(s.getData().index);
            }
            Thread.sleep(10);
        }
        assertEquals(List.of(4, 6), got);
        cft.setExpressionParameters("5");
        assertEquals(List.of("5"), cft.getExpressionParameters());
        assertThrows(RuntimeException.class, () -> cft.setExpressionParameters(List.of()));
        assertThrows(RuntimeException.class,
                () -> dp.createContentFilteredTopic("bad", topic, "nope = 1"));
    }

    @Test
    void requesterAndReplier() throws Exception {
        Topic<Greeting> requests = dp.createTopic("Echo_Request", Greeting.class);
        Topic<Greeting> replies = dp.createTopic("Echo_Reply", Greeting.class);
        try (Replier<Greeting, Greeting> replier = new Replier<>(requests, replies, null, null, "echo");
                Requester<Greeting, Greeting> requester = new Requester<>(requests, replies)) {
            requester.waitForService(tenSeconds);
            Thread server = new Thread(() -> {
                try {
                    for (int i = 0; i < 2; i++) {
                        Replier.Request<Greeting> r = replier.receiveRequest(tenSeconds);
                        replier.sendReply(new Greeting(r.data().index * 10, "re: " + r.data().message),
                                r.identity());
                    }
                } catch (TimeoutException e) {
                    throw new AssertionError(e);
                }
            });
            server.start();
            SampleIdentity first = requester.sendRequest(new Greeting(1, "one"));
            Greeting second = requester.request(new Greeting(2, "two"), tenSeconds);
            assertEquals(20, second.index);
            assertEquals("re: two", second.message);
            assertEquals(10, requester.receiveReply(first, tenSeconds).index);
            assertEquals(16, first.getWriterGuid().length);
            Duration short_ = Duration.newDuration(100, TimeUnit.MILLISECONDS, env);
            assertThrows(TimeoutException.class, () -> requester.receiveReply(first, short_));
            server.join();
        }
    }

    @Test
    void writeAndTake() throws Exception {
        Topic<Greeting> topic = dp.createTopic("greetings", Greeting.class);
        assertEquals("greetings", topic.getName());
        assertEquals("HelloWorld::Greeting", topic.getTypeName());
        assertSame(topic, dp.lookupTopicDescription("greetings"));
        Publisher pub = dp.createPublisher();
        Subscriber sub = dp.createSubscriber();
        DataWriter<Greeting> writer = pub.createDataWriter(topic, reliableWriter(pub));
        DataReader<Greeting> reader = sub.createDataReader(topic, reliableReader(sub));
        Rustorb.waitForSubscriptions(writer, 1, tenSeconds);
        Rustorb.waitForPublications(reader, 1, tenSeconds);
        for (int i = 1; i <= 3; i++) {
            writer.write(new Greeting(i, "hello " + i));
        }
        writer.waitForAcknowledgments(tenSeconds);
        Rustorb.waitForData(reader, tenSeconds);

        // read leaves them, and marks them read
        List<Sample<Greeting>> read = new ArrayList<>();
        reader.read(read);
        assertEquals(3, read.size());
        assertEquals(SampleState.NOT_READ, read.get(0).getSampleState());
        assertEquals(ViewState.NEW, read.get(0).getViewState());
        assertEquals(InstanceState.ALIVE, read.get(0).getInstanceState());
        assertTrue(read.get(0).getSourceTimestamp().isValid());
        reader.read(read);
        assertEquals(SampleState.READ, read.get(0).getSampleState());
        // select with max samples
        try (Sample.Iterator<Greeting> it = reader.select().maxSamples(2).read()) {
            int n = 0;
            while (it.hasNext()) {
                it.next();
                n++;
            }
            assertEquals(2, n);
        }
        List<Sample<Greeting>> taken = takeAll(reader);
        assertEquals(3, taken.size());
        for (int i = 0; i < 3; i++) {
            assertEquals(i + 1, taken.get(i).getData().index);
            assertEquals("hello " + (i + 1), taken.get(i).getData().message);
        }
        assertTrue(takeAll(reader).isEmpty());

        PublicationMatchedStatus pm = writer.getPublicationMatchedStatus();
        assertEquals(1, pm.getCurrentCount());
        SubscriptionMatchedStatus sm = reader.getSubscriptionMatchedStatus();
        assertEquals(1, sm.getCurrentCount());
        assertEquals(1, writer.getMatchedSubscriptions().size());
        assertEquals(1, reader.getMatchedPublications().size());
        assertSame(pub, writer.getParent());
        assertSame(topic, writer.getTopic());
        assertSame(writer, pub.lookupDataWriter("greetings"));
        assertSame(reader, sub.lookupDataReader("greetings"));
    }

    @Test
    void qos() {
        Topic<Greeting> topic = dp.createTopic("qos", Greeting.class);
        Publisher pub = dp.createPublisher();
        DataWriter<Greeting> writer = pub.createDataWriter(topic, pub.getDefaultDataWriterQos().withPolicies(
                policy.Reliability().withReliable(), policy.History().withKeepLast(7),
                policy.Durability().withTransientLocal(),
                policy.Deadline().withPeriod(2, TimeUnit.SECONDS)));
        DataWriterQos q = writer.getQos();
        assertEquals(Reliability.Kind.RELIABLE, q.getReliability().getKind());
        assertEquals(7, q.getHistory().getDepth());
        assertEquals(2000, q.getDeadline().getPeriod().getDuration(TimeUnit.MILLISECONDS));
        assertEquals(q.getReliability(), q.get(Reliability.class));
        assertThrows(UnsupportedOperationException.class, () -> writer.setQos(q));
        Subscriber sub = dp.createSubscriber();
        DataReader<Greeting> reader = sub.createDataReader(topic);
        assertEquals(Reliability.Kind.BEST_EFFORT, reader.getQos().getReliability().getKind());
    }

    @Test
    void keyedInstances() throws Exception {
        Topic<Shape> topic = dp.createTopic("shapes", Shape.class);
        Publisher pub = dp.createPublisher();
        Subscriber sub = dp.createSubscriber();
        DataWriter<Shape> writer = pub.createDataWriter(topic, reliableWriter(pub).withPolicy(
                policy.WriterDataLifecycle().withAutDisposeUnregisteredInstances(false)));
        DataReader<Shape> reader = sub.createDataReader(topic, reliableReader(sub));
        Rustorb.waitForSubscriptions(writer, 1, tenSeconds);

        InstanceHandle two = writer.registerInstance(new Shape(2, 0, ""));
        assertFalse(two.isNil());
        assertEquals(two, writer.lookupInstance(new Shape(2, 99, "other")));
        assertTrue(writer.lookupInstance(new Shape(5, 0, "")).isNil());
        assertEquals(2, writer.getKeyValue(two).id);
        for (int i = 1; i <= 6; i++) {
            writer.write(new Shape((i - 1) % 3 + 1, i, "s"));
        }
        writer.dispose(two);
        writer.unregisterInstance(writer.lookupInstance(new Shape(3, 0, "")));
        writer.waitForAcknowledgments(tenSeconds);

        InstanceHandle readerTwo = reader.lookupInstance(new Shape(2, 0, ""));
        assertFalse(readerTwo.isNil());
        assertEquals(2, reader.getKeyValue(null, readerTwo).id);
        // the samples of one instance
        try (Sample.Iterator<Shape> it = reader.select().instance(readerTwo).read()) {
            int n = 0;
            while (it.hasNext()) {
                Sample<Shape> s = it.next();
                assertEquals(readerTwo, s.getInstanceHandle());
                n++;
            }
            assertTrue(n >= 2);
        }
        boolean disposed = false;
        boolean noWriters = false;
        int valid = 0;
        for (Sample<Shape> s : takeAll(reader)) {
            if (s.getData() != null) {
                valid++;
            } else if (s.getInstanceHandle().equals(readerTwo)) {
                disposed |= s.getInstanceState() == InstanceState.NOT_ALIVE_DISPOSED;
            } else {
                noWriters |= s.getInstanceState() == InstanceState.NOT_ALIVE_NO_WRITERS;
            }
        }
        assertEquals(6, valid);
        assertTrue(disposed);
        assertTrue(noWriters);
    }

    @Test
    void listeners() throws Exception {
        Topic<Greeting> topic = dp.createTopic("listened", Greeting.class);
        Publisher pub = dp.createPublisher();
        Subscriber sub = dp.createSubscriber();
        CountDownLatch writerMatched = new CountDownLatch(1);
        DataWriter<Greeting> writer = pub.createDataWriter(topic, reliableWriter(pub),
                new DataWriterAdapter<Greeting>() {
                    @Override
                    public void onPublicationMatched(PublicationMatchedEvent<Greeting> e) {
                        if (e.getStatus().getCurrentCount() == 1) {
                            writerMatched.countDown();
                        }
                    }
                }, null);
        CountDownLatch matched = new CountDownLatch(1);
        CountDownLatch data = new CountDownLatch(3);
        List<Integer> got = new CopyOnWriteArrayList<>();
        DataReader<Greeting> reader = sub.createDataReader(topic, reliableReader(sub),
                new DataReaderAdapter<Greeting>() {
                    @Override
                    public void onSubscriptionMatched(SubscriptionMatchedEvent<Greeting> e) {
                        matched.countDown();
                    }

                    @Override
                    public void onDataAvailable(DataAvailableEvent<Greeting> e) {
                        DataReader<Greeting> r = e.getSource();
                        for (Sample<Greeting> s : takeAll(r)) {
                            got.add(s.getData().index);
                            data.countDown();
                        }
                    }
                }, null);
        assertTrue(matched.await(10, TimeUnit.SECONDS));
        assertTrue(writerMatched.await(10, TimeUnit.SECONDS));
        assertNotNull(reader.getListener());
        for (int i = 1; i <= 3; i++) {
            writer.write(new Greeting(i, "x"));
        }
        assertTrue(data.await(10, TimeUnit.SECONDS));
        assertEquals(List.of(1, 2, 3), got);
        // without a listener, no more calls
        reader.setListener(null);
        assertNull(reader.getListener());
        writer.write(new Greeting(4, "x"));
        Rustorb.waitForData(reader, tenSeconds);
        assertEquals(3, got.size());
    }

    @Test
    void conditionsAndWaitSets() throws Exception {
        Topic<Greeting> topic = dp.createTopic("waited", Greeting.class);
        Publisher pub = dp.createPublisher();
        Subscriber sub = dp.createSubscriber();
        DataWriter<Greeting> writer = pub.createDataWriter(topic, reliableWriter(pub));
        DataReader<Greeting> reader = sub.createDataReader(topic, reliableReader(sub));
        WaitSet ws = WaitSet.newWaitSet(env);

        // A guard condition.
        GuardCondition guard = GuardCondition.newGuardCondition(env);
        ws.attachCondition(guard);
        assertThrows(TimeoutException.class, () -> ws.waitForConditions(50, TimeUnit.MILLISECONDS));
        guard.setTriggerValue(true);
        List<Condition> active = new ArrayList<>();
        ws.waitForConditions(active, tenSeconds);
        assertEquals(List.of(guard), active);
        ws.detachCondition(guard);

        // A status condition.
        StatusCondition<DataReader<Greeting>> status = reader.getStatusCondition();
        status.setEnabledStatuses(List.<Class<? extends Status>>of(DataAvailableStatus.class));
        assertSame(reader, status.getParent());
        ws.attachCondition(status);
        Rustorb.waitForSubscriptions(writer, 1, tenSeconds);
        writer.write(new Greeting(1, "x"));
        ws.waitForConditions(active, tenSeconds);
        assertEquals(List.of(status), active);
        assertTrue(reader.getStatusChanges().contains(DataAvailableStatus.class));
        ws.detachCondition(status);

        // A read condition on samples not read.
        ReadCondition<Greeting> notRead = reader.createReadCondition(
                sub.createDataState().with(SampleState.NOT_READ).withAnyViewState().withAnyInstanceState());
        ws.attachCondition(notRead);
        ws.waitForConditions(active, tenSeconds);
        assertEquals(List.of(notRead), active);
        assertTrue(notRead.getTriggerValue());
        try (Sample.Iterator<Greeting> it = Rustorb.select(notRead).read()) {
            assertTrue(it.hasNext());
            assertEquals(1, it.next().getData().index);
        }
        assertFalse(notRead.getTriggerValue());
        assertThrows(TimeoutException.class, () -> ws.waitForConditions(50, TimeUnit.MILLISECONDS));
        assertEquals(1, ws.getConditions().size());
        notRead.close();
    }

    @Test
    void typeSupportAndClose() {
        TypeSupport<Greeting> ts = TypeSupport.newTypeSupport(Greeting.class, env);
        assertEquals("HelloWorld::Greeting", ts.getTypeName());
        assertEquals(Greeting.class, ts.getType());
        assertEquals(0, ts.newData().index);
        Topic<Greeting> topic = dp.createTopic("supported", ts);
        assertEquals("HelloWorld::Greeting", topic.getTypeName());
        // Types are registered under their IDL names only.
        assertThrows(UnsupportedOperationException.class,
                () -> TypeSupport.newTypeSupport(Greeting.class, "Renamed", env));
        assertThrows(IllegalArgumentException.class, () -> TypeSupport.newTypeSupport(String.class, env));

        Publisher pub = dp.createPublisher();
        DataWriter<Greeting> writer = pub.createDataWriter(topic);
        writer.close();
        assertThrows(AlreadyClosedException.class, () -> writer.write(new Greeting(1, "x")));
        // closing a participant closes what it made
        Subscriber sub = dp.createSubscriber();
        DataReader<Greeting> reader = sub.createDataReader(topic);
        dp.closeContainedEntities();
        assertThrows(AlreadyClosedException.class, reader::take);
        assertThrows(AlreadyClosedException.class, () -> pub.createDataWriter(topic));
    }
}

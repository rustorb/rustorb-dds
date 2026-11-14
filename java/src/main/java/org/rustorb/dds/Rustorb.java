// What rustorb-dds adds to the DDS Java 5 PSM.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import java.util.concurrent.TimeoutException;
import org.omg.dds.core.Duration;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.policy.DataRepresentation;
import org.omg.dds.domain.DomainParticipant;
import org.omg.dds.domain.DomainParticipantQos;
import org.omg.dds.pub.DataWriter;
import org.omg.dds.sub.DataReader;
import org.omg.dds.sub.ReadCondition;

/** rustorb extensions to the standard API. */
public final class Rustorb {
    /** DataRepresentationId XCDR2_DATA_REPRESENTATION (DDS-XTypes 7.6.3.1.1),
     * which the Java 5 PSM predates; use it with
     * {@code DataRepresentation.withValue(...)}. */
    public static final short XCDR2_DATA_REPRESENTATION = 2;

    private Rustorb() {
    }

    /** A participant that uses only the loopback interface (for tests). */
    public static DomainParticipant localhostParticipant(ServiceEnvironment env, int domainId) {
        return localhostParticipant(env, domainId, null);
    }

    /** A participant that uses only the loopback interface (for tests). */
    public static DomainParticipant localhostParticipant(ServiceEnvironment env, int domainId,
            DomainParticipantQos qos) {
        ParticipantImpl.Factory f = (ParticipantImpl.Factory) env.getSPI().getParticipantFactory();
        return f.create(domainId, qos, ParticipantImpl.LOCALHOST);
    }

    /** The number of other participants this one has discovered. */
    public static long discoveredParticipants(DomainParticipant participant) {
        return impl(participant).discoveredParticipants();
    }

    /** Waits until at least count readers match the writer. */
    public static void waitForSubscriptions(DataWriter<?> writer, int count, Duration timeout)
            throws TimeoutException {
        impl(writer).waitForSubscriptions(count, timeout);
    }

    /** Waits until at least count writers match the reader. */
    public static void waitForPublications(DataReader<?> reader, int count, Duration timeout)
            throws TimeoutException {
        impl(reader).waitForPublications(count, timeout);
    }

    /** Waits until the reader has a sample it has not read. */
    public static void waitForData(DataReader<?> reader, Duration timeout) throws TimeoutException {
        impl(reader).waitForData(timeout);
    }

    /** A selector of the samples of a read condition. */
    @SuppressWarnings("unchecked")
    public static <T> DataReader.Selector<T> select(ReadCondition<T> condition) {
        if (!(condition instanceof Conditions.Read<?> c)) {
            throw new IllegalArgumentException("not a rustorb read condition");
        }
        return ((ReaderImpl<T>) c.getParent()).select((Conditions.Read<T>) c);
    }

    private static ParticipantImpl impl(DomainParticipant p) {
        if (p instanceof ParticipantImpl i) {
            return i;
        }
        throw new IllegalArgumentException("not a rustorb participant");
    }

    private static WriterImpl<?> impl(DataWriter<?> w) {
        if (w instanceof WriterImpl<?> i) {
            return i;
        }
        throw new IllegalArgumentException("not a rustorb writer");
    }

    private static ReaderImpl<?> impl(DataReader<?> r) {
        if (r instanceof ReaderImpl<?> i) {
            return i;
        }
        throw new IllegalArgumentException("not a rustorb reader");
    }
}

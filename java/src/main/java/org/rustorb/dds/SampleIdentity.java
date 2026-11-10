// The identity of a sample (DDS-RPC 1.0, 7.5.1.1.1).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.MemoryLayout;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.StructLayout;
import java.lang.foreign.ValueLayout;
import java.util.Arrays;

/** The identity of a sample: its writer's GUID and its sequence number. A
 * request's identity is what its replies name. */
public final class SampleIdentity {
    /** rdds_sample_identity. */
    static final StructLayout LAYOUT = MemoryLayout.structLayout(
            MemoryLayout.sequenceLayout(16, ValueLayout.JAVA_BYTE).withName("writer"),
            JAVA_LONG.withName("sequence_number"));

    private final byte[] writerGuid;
    private final long sequenceNumber;

    /** From the 16 octets of a GUID and a sequence number. */
    public SampleIdentity(byte[] writerGuid, long sequenceNumber) {
        if (writerGuid.length != 16) {
            throw new IllegalArgumentException("a GUID has 16 octets");
        }
        this.writerGuid = writerGuid.clone();
        this.sequenceNumber = sequenceNumber;
    }

    static SampleIdentity of(MemorySegment s) {
        return new SampleIdentity(s.asSlice(0, 16).toArray(ValueLayout.JAVA_BYTE), s.get(JAVA_LONG, 16));
    }

    void put(MemorySegment s) {
        MemorySegment.copy(writerGuid, 0, s, ValueLayout.JAVA_BYTE, 0, 16);
        s.set(JAVA_LONG, 16, sequenceNumber);
    }

    /** The GUID of the writer, 16 octets. */
    public byte[] getWriterGuid() {
        return writerGuid.clone();
    }

    /** The sequence number. */
    public long getSequenceNumber() {
        return sequenceNumber;
    }

    @Override
    public boolean equals(Object o) {
        return o instanceof SampleIdentity other && sequenceNumber == other.sequenceNumber
                && Arrays.equals(writerGuid, other.writerGuid);
    }

    @Override
    public int hashCode() {
        return 31 * Arrays.hashCode(writerGuid) + Long.hashCode(sequenceNumber);
    }

    @Override
    public String toString() {
        StringBuilder b = new StringBuilder("SampleIdentity(");
        for (byte x : writerGuid) {
            b.append(String.format("%02x", x));
        }
        return b.append(", ").append(sequenceNumber).append(')').toString();
    }
}

// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

import java.util.List;

/** DATA_REPRESENTATION (DDS-XTypes 1.3, 7.6.3.1). XCDR2 is 2
 * (org.rustorb.dds.Rustorb.XCDR2_DATA_REPRESENTATION). */
public interface DataRepresentation extends QosPolicy.ForTopic, QosPolicy.ForDataReader, QosPolicy.ForDataWriter {
    public List<Short> getValue();

    public DataRepresentation withValue(List<Short> value);

    public DataRepresentation withValue(short... value);

    /** The representation identifiers. */
    public static final class Id {
        private Id() {}

        public static final short XCDR_DATA_REPRESENTATION = 0;
        public static final short XML_DATA_REPRESENTATION = 1;
    }
}

// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

/** HISTORY. */
public interface History extends QosPolicy.ForTopic, QosPolicy.ForDataReader, QosPolicy.ForDataWriter {
    public Kind getKind();

    public int getDepth();

    public History withKind(Kind kind);

    public History withDepth(int depth);

    public History withKeepAll();

    public History withKeepLast(int depth);

    public enum Kind {
        KEEP_LAST,
        KEEP_ALL
    }
}

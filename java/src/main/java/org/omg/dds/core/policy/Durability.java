// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

/** DURABILITY (rustorb-dds offers VOLATILE and TRANSIENT_LOCAL). */
public interface Durability extends QosPolicy.ForTopic, QosPolicy.ForDataReader, QosPolicy.ForDataWriter {
    public Kind getKind();

    public Durability withKind(Kind kind);

    public Durability withVolatile();

    public Durability withTransientLocal();

    public Durability withTransient();

    /** Spelled as in the PSM. */
    public Durability withPersitent();

    public enum Kind {
        VOLATILE,
        TRANSIENT_LOCAL,
        TRANSIENT,
        PERSISTENT
    }
}

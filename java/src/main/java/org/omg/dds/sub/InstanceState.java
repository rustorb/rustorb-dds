// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.sub;

/** The instance state of a sample's instance. */
public enum InstanceState {
    ALIVE(0x0001 << 0),
    NOT_ALIVE_DISPOSED(0x0001 << 1),
    NOT_ALIVE_NO_WRITERS(0x0001 << 2);

    /** The bit of the state (DDS 1.4, 2.2.2.5.1.8). */
    public final int value;

    private InstanceState(int value) {
        this.value = value;
    }
}

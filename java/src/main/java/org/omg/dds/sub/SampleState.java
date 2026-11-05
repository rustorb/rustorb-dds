// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.sub;

/** The sample state of a sample. */
public enum SampleState {
    READ(0x0001 << 0),
    NOT_READ(0x0001 << 1);

    /** The bit of the state (DDS 1.4, 2.2.2.5.1.8). */
    public final int value;

    private SampleState(int value) {
        this.value = value;
    }
}

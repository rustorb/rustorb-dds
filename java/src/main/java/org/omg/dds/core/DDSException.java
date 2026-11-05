// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

/** A failure the DDS service reports (RETCODE_ERROR), and the base of the
 * other exceptions of the PSM. */
public abstract class DDSException extends RuntimeException implements DDSObject {
    private static final long serialVersionUID = 1L;

    protected DDSException() {
        super();
    }

    protected DDSException(String message) {
        super(message);
    }

    protected DDSException(Throwable cause) {
        super(cause);
    }

    protected DDSException(String message, Throwable cause) {
        super(message, cause);
    }
}

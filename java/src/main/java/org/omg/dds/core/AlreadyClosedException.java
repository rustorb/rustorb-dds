// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

/** RETCODE_ALREADY_DELETED. */
public abstract class AlreadyClosedException extends DDSException {
    private static final long serialVersionUID = 1L;

    protected AlreadyClosedException() {
        super();
    }

    protected AlreadyClosedException(String message) {
        super(message);
    }

    protected AlreadyClosedException(Throwable cause) {
        super(cause);
    }

    protected AlreadyClosedException(String message, Throwable cause) {
        super(message, cause);
    }
}

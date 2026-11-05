// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

/** The DDS implementation failed to initialize. */
public class ServiceInitializationException extends RuntimeException {
    private static final long serialVersionUID = 1L;

    public ServiceInitializationException() {
        super();
    }

    public ServiceInitializationException(String message) {
        super(message);
    }

    public ServiceInitializationException(Throwable cause) {
        super(cause);
    }

    public ServiceInitializationException(String message, Throwable cause) {
        super(message, cause);
    }
}

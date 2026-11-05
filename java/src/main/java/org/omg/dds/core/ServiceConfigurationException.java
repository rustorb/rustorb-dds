// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

/** The DDS implementation could not be found or loaded. */
public class ServiceConfigurationException extends RuntimeException {
    private static final long serialVersionUID = 1L;

    public ServiceConfigurationException() {
        super();
    }

    public ServiceConfigurationException(String message) {
        super(message);
    }

    public ServiceConfigurationException(Throwable cause) {
        super(cause);
    }

    public ServiceConfigurationException(String message, Throwable cause) {
        super(message, cause);
    }
}

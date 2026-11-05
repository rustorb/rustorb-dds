// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

/** RETCODE_IMMUTABLE_POLICY. */
public abstract class ImmutablePolicyException extends DDSException {
    private static final long serialVersionUID = 1L;

    protected ImmutablePolicyException() {
        super();
    }

    protected ImmutablePolicyException(String message) {
        super(message);
    }

    protected ImmutablePolicyException(Throwable cause) {
        super(cause);
    }

    protected ImmutablePolicyException(String message, Throwable cause) {
        super(message, cause);
    }
}

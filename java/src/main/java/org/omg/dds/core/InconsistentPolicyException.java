// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

/** RETCODE_INCONSISTENT_POLICY. */
public abstract class InconsistentPolicyException extends DDSException {
    private static final long serialVersionUID = 1L;

    protected InconsistentPolicyException() {
        super();
    }

    protected InconsistentPolicyException(String message) {
        super(message);
    }

    protected InconsistentPolicyException(Throwable cause) {
        super(cause);
    }

    protected InconsistentPolicyException(String message, Throwable cause) {
        super(message, cause);
    }
}

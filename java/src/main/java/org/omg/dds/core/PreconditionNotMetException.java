// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

/** RETCODE_PRECONDITION_NOT_MET. */
public abstract class PreconditionNotMetException extends DDSException {
    private static final long serialVersionUID = 1L;

    protected PreconditionNotMetException() {
        super();
    }

    protected PreconditionNotMetException(String message) {
        super(message);
    }

    protected PreconditionNotMetException(Throwable cause) {
        super(cause);
    }

    protected PreconditionNotMetException(String message, Throwable cause) {
        super(message, cause);
    }
}

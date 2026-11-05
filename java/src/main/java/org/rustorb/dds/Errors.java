// The return codes of the C ABI as the exceptions of the PSM (Java 5 PSM
// for DDS, 7.3.2, Table 7.1).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import java.util.concurrent.TimeoutException;

import org.omg.dds.core.AlreadyClosedException;
import org.omg.dds.core.DDSException;
import org.omg.dds.core.IllegalOperationException;
import org.omg.dds.core.ImmutablePolicyException;
import org.omg.dds.core.InconsistentPolicyException;
import org.omg.dds.core.NotEnabledException;
import org.omg.dds.core.OutOfResourcesException;
import org.omg.dds.core.PreconditionNotMetException;
import org.omg.dds.core.ServiceEnvironment;

final class Errors {
    private Errors() {}

    static final int OK = 0;
    static final int ERROR = 1;
    static final int UNSUPPORTED = 2;
    static final int BAD_PARAMETER = 3;
    static final int TIMEOUT = 10;
    static final int NO_DATA = 11;

    /** The message of the last failure on this thread. */
    static String lastMessage() {
        try {
            return Library.cString((long) Ffi.rdds_last_error_message.invokeExact());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** Throws the exception of a return code other than OK (TIMEOUT as a
     * DDSException: see {@link #checkTimeout}). */
    static void check(ServiceEnvironment env, int rc) {
        if (rc != OK) {
            throw exception(env, rc, lastMessage());
        }
    }

    /** check, with TIMEOUT as java.util.concurrent.TimeoutException. */
    static void checkTimeout(ServiceEnvironment env, int rc) throws TimeoutException {
        if (rc == TIMEOUT) {
            throw new TimeoutException(lastMessage());
        }
        check(env, rc);
    }

    static RuntimeException exception(ServiceEnvironment env, int rc, String message) {
        switch (rc) {
            case BAD_PARAMETER:
                return new IllegalArgumentException(message);
            case UNSUPPORTED:
                return new UnsupportedOperationException(message);
            case 9:
                return new AlreadyClosed(env, message);
            case 12:
                return new IllegalOperation(env, message);
            case 7:
                return new ImmutablePolicy(env, message);
            case 8:
                return new InconsistentPolicy(env, message);
            case 6:
                return new NotEnabled(env, message);
            case 5:
                return new OutOfResources(env, message);
            case 4:
                return new PreconditionNotMet(env, message);
            default:
                return new Error(env, message);
        }
    }

    static AlreadyClosedException closed(ServiceEnvironment env, String what) {
        return new AlreadyClosed(env, what + " is closed");
    }

    static PreconditionNotMetException precondition(ServiceEnvironment env, String message) {
        return new PreconditionNotMet(env, message);
    }

    static final class Error extends DDSException {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;

        Error(ServiceEnvironment env, String message) {
            super(message);
            this.env = env;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }
    }

    static final class AlreadyClosed extends AlreadyClosedException {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;

        AlreadyClosed(ServiceEnvironment env, String message) {
            super(message);
            this.env = env;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }
    }

    static final class IllegalOperation extends IllegalOperationException {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;

        IllegalOperation(ServiceEnvironment env, String message) {
            super(message);
            this.env = env;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }
    }

    static final class ImmutablePolicy extends ImmutablePolicyException {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;

        ImmutablePolicy(ServiceEnvironment env, String message) {
            super(message);
            this.env = env;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }
    }

    static final class InconsistentPolicy extends InconsistentPolicyException {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;

        InconsistentPolicy(ServiceEnvironment env, String message) {
            super(message);
            this.env = env;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }
    }

    static final class NotEnabled extends NotEnabledException {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;

        NotEnabled(ServiceEnvironment env, String message) {
            super(message);
            this.env = env;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }
    }

    static final class OutOfResources extends OutOfResourcesException {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;

        OutOfResources(ServiceEnvironment env, String message) {
            super(message);
            this.env = env;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }
    }

    static final class PreconditionNotMet extends PreconditionNotMetException {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;

        PreconditionNotMet(ServiceEnvironment env, String message) {
            super(message);
            this.env = env;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }
    }
}

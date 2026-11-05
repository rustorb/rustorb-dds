// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.status;

import java.io.Serializable;
import java.util.Set;
import org.omg.dds.core.DDSObject;
import org.omg.dds.core.ServiceEnvironment;

/** A communication status of an entity. Its class is its status kind;
 * a set of classes is a status mask. */
public abstract class Status implements Serializable, DDSObject {
    private static final long serialVersionUID = 1L;

    /** Every status kind. */
    public static Set<Class<? extends Status>> allStatuses(ServiceEnvironment env) {
        return env.getSPI().allStatusKinds();
    }

    /** No status kind. */
    public static Set<Class<? extends Status>> noStatuses(ServiceEnvironment env) {
        return env.getSPI().noStatusKinds();
    }
}

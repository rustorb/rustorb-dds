// Status kinds as classes (the PSM) and as bits (the C ABI).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import java.util.Collection;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.Map;
import java.util.Set;

import org.omg.dds.core.status.DataAvailableStatus;
import org.omg.dds.core.status.DataOnReadersStatus;
import org.omg.dds.core.status.InconsistentTopicStatus;
import org.omg.dds.core.status.LivelinessChangedStatus;
import org.omg.dds.core.status.LivelinessLostStatus;
import org.omg.dds.core.status.OfferedDeadlineMissedStatus;
import org.omg.dds.core.status.OfferedIncompatibleQosStatus;
import org.omg.dds.core.status.PublicationMatchedStatus;
import org.omg.dds.core.status.RequestedDeadlineMissedStatus;
import org.omg.dds.core.status.RequestedIncompatibleQosStatus;
import org.omg.dds.core.status.SampleLostStatus;
import org.omg.dds.core.status.SampleRejectedStatus;
import org.omg.dds.core.status.Status;
import org.omg.dds.core.status.SubscriptionMatchedStatus;

final class StatusKinds {
    private StatusKinds() {}

    static final int ALL = 0xffffffff;

    /** StatusKind bits (DDS 1.4, 2.2.4.1). */
    static final Map<Class<? extends Status>, Integer> BITS = new LinkedHashMap<>();

    static {
        BITS.put(InconsistentTopicStatus.class, 1 << 0);
        BITS.put(OfferedDeadlineMissedStatus.class, 1 << 1);
        BITS.put(RequestedDeadlineMissedStatus.class, 1 << 2);
        BITS.put(OfferedIncompatibleQosStatus.class, 1 << 5);
        BITS.put(RequestedIncompatibleQosStatus.class, 1 << 6);
        BITS.put(SampleLostStatus.class, 1 << 7);
        BITS.put(SampleRejectedStatus.class, 1 << 8);
        BITS.put(DataOnReadersStatus.class, 1 << 9);
        BITS.put(DataAvailableStatus.class, 1 << 10);
        BITS.put(LivelinessLostStatus.class, 1 << 11);
        BITS.put(LivelinessChangedStatus.class, 1 << 12);
        BITS.put(PublicationMatchedStatus.class, 1 << 13);
        BITS.put(SubscriptionMatchedStatus.class, 1 << 14);
    }

    static Set<Class<? extends Status>> all() {
        return Collections.unmodifiableSet(new LinkedHashSet<>(BITS.keySet()));
    }

    /** The mask of the statuses (null: all of them). */
    static int mask(Collection<Class<? extends Status>> statuses) {
        if (statuses == null) {
            return ALL;
        }
        int m = 0;
        for (Class<? extends Status> c : statuses) {
            for (Map.Entry<Class<? extends Status>, Integer> e : BITS.entrySet()) {
                if (e.getKey().isAssignableFrom(c)) {
                    m |= e.getValue();
                }
            }
        }
        return m;
    }

    static Set<Class<? extends Status>> of(int mask) {
        Set<Class<? extends Status>> s = new LinkedHashSet<>();
        for (Map.Entry<Class<? extends Status>, Integer> e : BITS.entrySet()) {
            if ((mask & e.getValue()) != 0) {
                s.add(e.getKey());
            }
        }
        return Collections.unmodifiableSet(s);
    }
}

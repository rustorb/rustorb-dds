// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

import java.util.Map;

/**
 * PROPERTY of a DomainParticipant (DDS Security 1.2, 7.2.5; the Java PSM
 * has no such policy, so this one is rustorb-dds's). The {@code dds.sec.*}
 * properties of the builtin plugins turn on DDS Security: all six are
 * needed, with {@code file:} or {@code data:,} URIs as values. Other
 * properties are ignored.
 */
public interface Property extends QosPolicy.ForDomainParticipant {
    /** A copy of the properties, in the order they were set. */
    public Map<String, String> getValue();

    /** The value of a property, or null. */
    public String get(String name);

    /** These properties instead. */
    public Property withValue(Map<String, String> value);

    /** One more property, or a new value for one. */
    public Property with(String name, String value);
}

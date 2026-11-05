// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.type;

import org.omg.dds.core.DDSObject;
import org.omg.dds.core.ServiceEnvironment;

/**
 * The type of a topic. rustorb-dds finds the type support of a class that
 * rustorb-idl generated code for (-bjava -Wbdds or -Wbdds_only): the class
 * {@code <Name>Codec} next to the class of the type.
 */
public abstract class TypeSupport<TYPE> implements DDSObject {
    /** The type support of the class, registered under the name of its IDL
     * type ("Module::Name"), as other DDS implementations name it. */
    public static <TYPE> TypeSupport<TYPE> newTypeSupport(Class<TYPE> type, ServiceEnvironment env) {
        return env.getSPI().newTypeSupport(type, null);
    }

    /** The type support of the class, registered under the name given. */
    public static <TYPE> TypeSupport<TYPE> newTypeSupport(
            Class<TYPE> type, String registeredName, ServiceEnvironment env) {
        return env.getSPI().newTypeSupport(type, registeredName);
    }

    /** A new value of the type, with the default values of its members. */
    public abstract TYPE newData();

    public abstract Class<TYPE> getType();

    public abstract String getTypeName();
}

// The type support of a Java class: what rustorb-idl generates for each
// struct, union, enum and bitmask (-bjava -Wbdds or -Wbdds_only).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

/**
 * Builds the DDS-XTypes type of a Java class and writes and reads its
 * values, in the order of the type, to and from the sample streams of
 * the C ABI (see docs/c-abi.md, "Samples"); rustorb-dds encodes them.
 * rustorb-idl generates {@code <Name>Codec} next to the class of each
 * type; {@code TypeSupport.newTypeSupport(Name.class, env)} finds it.
 */
public interface Codec<T> {
    /** The DDS type (built once). */
    public TypeRef getType();

    /** The name of the IDL type, "Module::Name". */
    public String getTypeName();

    public Class<T> getJavaClass();

    /** A new value with the default values of the members. */
    public T newValue();

    /** Writes the values of v. */
    public void write(Out out, T v);

    /** Reads a value. */
    public T read(In in);
}

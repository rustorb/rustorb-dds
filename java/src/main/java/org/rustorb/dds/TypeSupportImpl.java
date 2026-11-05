// The TypeSupport of a class rustorb-idl generated a Codec for.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.type.TypeSupport;

final class TypeSupportImpl<T> extends TypeSupport<T> {
    private final ServiceEnvironment env;
    final Codec<T> codec;
    private final String name;

    TypeSupportImpl(ServiceEnvironment env, Codec<T> codec, String name) {
        this.env = env;
        this.codec = codec;
        if (name != null && !name.equals(codec.getTypeName())) {
            // The C ABI registers a type under the name of its type.
            throw new UnsupportedOperationException("rustorb-dds registers " + codec.getTypeName()
                    + " under its IDL name, not " + name);
        }
        this.name = codec.getTypeName();
    }

    /** The Codec rustorb-idl generated for the class: the class
     * {@code <Name>Codec} next to it, whose INSTANCE is the codec. */
    @SuppressWarnings("unchecked")
    static <T> Codec<T> codecOf(Class<T> type) {
        String name = type.getName() + "Codec";
        try {
            Class<?> c = Class.forName(name, true, type.getClassLoader());
            Object codec = c.getField("INSTANCE").get(null);
            if (!(codec instanceof Codec<?> k) || k.getJavaClass() != type) {
                throw new IllegalArgumentException(name + ".INSTANCE is not a Codec of " + type.getName());
            }
            return (Codec<T>) codec;
        } catch (ClassNotFoundException e) {
            throw new IllegalArgumentException(type.getName() + " has no DDS type support (" + name
                    + "): generate it with rustorb-idl -bjava -Wbdds or -Wbdds_only", e);
        } catch (ReflectiveOperationException e) {
            throw new IllegalArgumentException("cannot use " + name, e);
        }
    }

    @Override
    public ServiceEnvironment getEnvironment() {
        return env;
    }

    @Override
    public T newData() {
        return codec.newValue();
    }

    @Override
    public Class<T> getType() {
        return codec.getJavaClass();
    }

    @Override
    public String getTypeName() {
        return name;
    }
}

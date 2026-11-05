// The InstanceHandle of the PSM: the 16 octets of an rdds_instance_handle
// or rdds_guid.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_BYTE;

import java.lang.foreign.MemorySegment;
import java.util.Arrays;
import java.util.HexFormat;

import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.ModifiableInstanceHandle;
import org.omg.dds.core.ServiceEnvironment;

final class HandleImpl extends ModifiableInstanceHandle {
    private static final long serialVersionUID = 1L;

    private final transient ServiceEnvironment env;
    private byte[] value;

    HandleImpl(ServiceEnvironment env, byte[] value) {
        this.env = env;
        this.value = value.clone();
    }

    static HandleImpl nil(ServiceEnvironment env) {
        return new HandleImpl(env, new byte[16]);
    }

    /** The handle in the 16 octets at offset of s. */
    static HandleImpl get(ServiceEnvironment env, MemorySegment s, long offset) {
        return new HandleImpl(env, s.asSlice(offset, 16).toArray(JAVA_BYTE));
    }

    /** The 16 octets of any InstanceHandle (null: nil). */
    static byte[] bytes(InstanceHandle h) {
        if (h == null) {
            return new byte[16];
        }
        if (h instanceof HandleImpl i) {
            return i.value;
        }
        if (h.isNil()) {
            return new byte[16];
        }
        throw new IllegalArgumentException("an InstanceHandle of another DDS implementation");
    }

    static void put(MemorySegment s, long offset, InstanceHandle h) {
        MemorySegment.copy(bytes(h), 0, s, JAVA_BYTE, offset, 16);
    }

    @Override
    public ServiceEnvironment getEnvironment() {
        return env;
    }

    @Override
    public boolean isNil() {
        for (byte b : value) {
            if (b != 0) {
                return false;
            }
        }
        return true;
    }

    @Override
    public ModifiableInstanceHandle modifiableCopy() {
        return new HandleImpl(env, value);
    }

    @Override
    public void copyFrom(InstanceHandle src) {
        value = bytes(src).clone();
    }

    @Override
    public InstanceHandle immutableCopy() {
        return new HandleImpl(env, value);
    }

    @Override
    public int compareTo(InstanceHandle o) {
        return Arrays.compareUnsigned(value, bytes(o));
    }

    @Override
    public boolean equals(Object o) {
        return o instanceof InstanceHandle h && Arrays.equals(value, bytes(h));
    }

    @Override
    public int hashCode() {
        return Arrays.hashCode(value);
    }

    @Override
    public String toString() {
        return isNil() ? "InstanceHandle(nil)" : "InstanceHandle(" + HexFormat.of().formatHex(value) + ")";
    }
}

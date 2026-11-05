// Loads librustorb_dds_ffi (crates/rustorb-dds-ffi) and binds its
// functions with the FFM API (java.lang.foreign), for Ffi.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_BYTE;

import java.io.File;
import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.nio.charset.StandardCharsets;

@SuppressWarnings("restricted")
final class Library {
    private Library() {}

    static final Linker LINKER = Linker.nativeLinker();

    /** Short calls that neither call back nor block skip the thread state
     * transition; those with heap segments also read Java arrays in place. */
    static final Linker.Option CRITICAL = Linker.Option.critical(false);
    static final Linker.Option CRITICAL_HEAP = Linker.Option.critical(true);

    /** All of memory, to read what the C ABI returns at an address. */
    static final MemorySegment MEMORY = MemorySegment.NULL.reinterpret(Long.MAX_VALUE);

    private static final SymbolLookup LIB = lookup();

    /**
     * The library at the path in the system property
     * {@code rustorb.dds.ffi.library} or the environment variable
     * {@code RUSTORB_DDS_FFI_LIBRARY}, else {@code rustorb_dds_ffi} from
     * {@code java.library.path}, else from the paths of the system's
     * dynamic loader.
     */
    private static SymbolLookup lookup() {
        String path = System.getProperty("rustorb.dds.ffi.library");
        if (path == null || path.isEmpty()) {
            path = System.getenv("RUSTORB_DDS_FFI_LIBRARY");
        }
        String name = System.mapLibraryName("rustorb_dds_ffi");
        if (path == null || path.isEmpty()) {
            String dirs = System.getProperty("java.library.path", "");
            for (String dir : dirs.split(File.pathSeparator)) {
                if (!dir.isEmpty() && new File(dir, name).isFile()) {
                    path = new File(dir, name).getPath();
                    break;
                }
            }
        }
        try {
            if (path != null && !path.isEmpty()) {
                return SymbolLookup.libraryLookup(new File(path).toPath().toAbsolutePath(), Arena.global());
            }
            return SymbolLookup.libraryLookup(name, Arena.global());
        } catch (IllegalArgumentException e) {
            UnsatisfiedLinkError u = new UnsatisfiedLinkError(
                    "cannot load " + name + " (set rustorb.dds.ffi.library to its path): " + e.getMessage());
            u.initCause(e);
            throw u;
        }
    }

    static MethodHandle fn(String name, FunctionDescriptor fd, Linker.Option... options) {
        MemorySegment symbol = LIB.find(name)
                .orElseThrow(() -> new UnsatisfiedLinkError("librustorb_dds_ffi has no " + name));
        return LINKER.downcallHandle(symbol, fd, options);
    }

    /** The address of an upcall stub of the static method name of the class
     * of lookup, alive for good. */
    static long upcall(MethodHandles.Lookup lookup, String name, FunctionDescriptor fd) {
        try {
            MethodHandle target = lookup.findStatic(lookup.lookupClass(), name, fd.toMethodType());
            return LINKER.upcallStub(target, fd, Arena.global()).address();
        } catch (ReflectiveOperationException e) {
            throw new ExceptionInInitializerError(e);
        }
    }

    /** The UTF-8 string of len bytes at p (malformed bytes replaced). */
    static String utf8(long p, long len) {
        if (p == 0 || len <= 0) {
            return "";
        }
        byte[] b = new byte[Math.toIntExact(len)];
        MemorySegment.copy(MEMORY, JAVA_BYTE, p, b, 0, b.length);
        return new String(b, StandardCharsets.UTF_8);
    }

    /** The NUL-terminated UTF-8 string at p. */
    static String cString(long p) {
        if (p == 0) {
            return "";
        }
        return MEMORY.getString(p);
    }

    /** Rethrows what MethodHandle.invokeExact threw (the C functions throw
     * nothing; the upcalls catch all). */
    static RuntimeException rethrow(Throwable t) {
        if (t instanceof RuntimeException r) {
            throw r;
        }
        if (t instanceof Error e) {
            throw e;
        }
        throw new IllegalStateException(t);
    }
}

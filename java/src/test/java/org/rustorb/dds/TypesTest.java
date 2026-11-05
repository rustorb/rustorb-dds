// The DDS type support rustorb-idl writes for Java (src/test/generated):
// the types it builds hash as Cyclone DDS's idlc describes them, and
// samples go through XCDR1 and XCDR2 and back unchanged.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.BitSet;
import java.util.HexFormat;
import org.junit.jupiter.api.Test;

class TypesTest {
    static final Path FIXTURES = Path.of(System.getProperty("rustorb.dds.fixtures",
            "../crates/rustorb-xtypes/tests/fixtures/cyclonedds"));

    /** The minimal and complete hashes of a fixture's type, from idlc's
     * TypeInformation (as cpp/tests/test_types.cc reads them). */
    static String[] idlcHashes(String name) throws IOException {
        String text = Files.readString(FIXTURES.resolve(name + ".typeinfo")).replaceAll("\\s", "");
        byte[] data = HexFormat.of().parseHex(text.length() % 2 == 0 ? text : text.substring(0, text.length() - 1));
        int i = find(data, new byte[] {(byte) 0xf1}, 0);
        int j = find(data, new byte[] {(byte) 0xf2}, find(data, new byte[] {0x02, 0x10, 0x00, 0x40}, 0));
        return new String[] {hex(data, i + 1), hex(data, j + 1)};
    }

    static int find(byte[] data, byte[] pattern, int from) {
        for (int i = from; i + pattern.length <= data.length; i++) {
            if (Arrays.equals(data, i, i + pattern.length, pattern, 0, pattern.length)) {
                return i;
            }
        }
        throw new AssertionError("pattern not found");
    }

    static String hex(byte[] b, int at) {
        return HexFormat.of().formatHex(b, at, at + 14);
    }

    static void checkHashes(Codec<?> codec, String fixture) throws IOException {
        String[] expected = idlcHashes(fixture);
        TypeRef t = codec.getType();
        assertEquals(expected[0] + " " + expected[1], hex(t.minimalHash(), 0) + " " + hex(t.completeHash(), 0),
                fixture);
    }

    static <T> byte[] serialize(Codec<T> codec, T v, boolean xcdr2) {
        try (Out out = new Out()) {
            codec.write(out, v);
            return codec.getType().serialize(out, xcdr2);
        }
    }

    static <T> T deserialize(Codec<T> codec, byte[] data) {
        try (In in = codec.getType().deserialize(data)) {
            T v = codec.read(in);
            in.check();
            return v;
        }
    }

    /** v through XCDR1 and XCDR2 and back: the same encoding again, and
     * the value read. */
    static <T> T roundTrip(Codec<T> codec, T v) {
        T back = null;
        for (boolean xcdr2 : new boolean[] {false, true}) {
            byte[] data = serialize(codec, v, xcdr2);
            back = deserialize(codec, data);
            assertArrayEquals(data, serialize(codec, back, xcdr2));
        }
        return back;
    }

    static Gen.Inner inner(int id) {
        return new Gen.Inner(id, "x".repeat(id + 1));
    }

    static Gen.All all() {
        Gen.All a = new Gen.All();
        a.color = Gen.Color.BLUE;
        a.perm.set(Gen.PermFlags.READ.getPosition());
        a.perm.set(Gen.PermFlags.EXEC.getPosition());
        a.ls = new int[] {1, 2, 3};
        a.name = "name";
        a.m = new int[][] {{1, 2, 3}, {4, 5, 6}};
        a.inners = new Gen.Inner[] {inner(1), inner(2)};
        a.ss = new int[][] {{1}, {}, {2, 3}};
        a.arr = new Gen.Inner[] {inner(4), inner(5), inner(6), inner(7)};
        for (int i = 0; i < 300; i++) {
            a.big_arr[i] = i * 7;
        }
        a.strs = new String[] {"a", "", "ccc"};
        a.u.s("three");
        a.ue.n("blue");
        a.mut.a = 5;
        a.mut.b = "bee";
        a.mut.k = 9;
        a.mut.inner = inner(8);
        a.h = new Gen.Hashed(1, "two", 3);
        a.p = new Gen.Plain(true, (byte) 0xfe, (short) -2, (short) 3, -4, 5, -6, 7, 1.5f, -2.25, 'z');
        a.ext = inner(9);
        a.opt = 42;
        return a;
    }

    @Test
    void hashesLikeIdlc() throws IOException {
        checkHashes(Gen.PlainCodec.INSTANCE, "Gen_Plain");
        checkHashes(Gen.InnerCodec.INSTANCE, "Gen_Inner");
        checkHashes(Gen.MutCodec.INSTANCE, "Gen_Mut");
        checkHashes(Gen.UCodec.INSTANCE, "Gen_U");
        checkHashes(Gen.UECodec.INSTANCE, "Gen_UE");
        checkHashes(Gen.HashedCodec.INSTANCE, "Gen_Hashed");
        checkHashes(Gen.AllCodec.INSTANCE, "Gen_All");
        checkHashes(Gen.TopCodec.INSTANCE, "Gen_Top");
        checkHashes(HelloWorld.GreetingCodec.INSTANCE, "HelloWorld_Greeting");
        checkHashes(Keyed.ShapeCodec.INSTANCE, "Keyed_Shape");
    }

    @Test
    void roundTripEverything() {
        Gen.Top top = roundTrip(Gen.TopCodec.INSTANCE, new Gen.Top(7, all()));
        assertEquals(7, top.id);
        Gen.All a = top.all;
        assertEquals(Gen.Color.BLUE, a.color);
        assertTrue(a.perm.get(0) && !a.perm.get(1) && a.perm.get(5));
        assertArrayEquals(new int[] {1, 2, 3}, a.ls);
        assertEquals(6, a.m[1][2]);
        assertEquals("xxx", a.inners[1].big);
        assertArrayEquals(new int[] {2, 3}, a.ss[2]);
        assertEquals(7 * 299, a.big_arr[299]);
        assertEquals("three", a.u.s());
        assertEquals(3, a.u.discriminator());
        assertEquals("blue", a.ue.n());
        assertEquals("bee", a.mut.b);
        assertEquals("two", a.h.beta);
        assertEquals('z', a.p.c);
        assertEquals((byte) 0xfe, a.p.o);
        assertEquals(Integer.valueOf(42), a.opt);

        // The other members of the unions, and absent optionals.
        Gen.All b = all();
        b.u.x(2, 5);
        b.ue.r(0.5);
        b.opt = null;
        b.mut.b = null;
        Gen.All back = roundTrip(Gen.TopCodec.INSTANCE, new Gen.Top(1, b)).all;
        assertEquals(2, back.u.discriminator());
        assertEquals(5, back.u.x());
        assertEquals(0.5, back.ue.r());
        assertNull(back.opt);
        assertNull(back.mut.b);
        b.u.i(-4, inner(3));
        back = roundTrip(Gen.TopCodec.INSTANCE, new Gen.Top(2, b)).all;
        assertEquals(-4, back.u.discriminator());
        assertEquals(3, back.u.i().id);

        // The default value, and the codec's.
        roundTrip(Gen.TopCodec.INSTANCE, new Gen.Top());
        roundTrip(Gen.TopCodec.INSTANCE, Gen.TopCodec.INSTANCE.newValue());
        assertEquals("Gen::Top", Gen.TopCodec.INSTANCE.getTypeName());
        assertEquals(Gen.Top.class, Gen.TopCodec.INSTANCE.getJavaClass());
    }

    @Test
    void roundTripWideAndMore() {
        CppTypes.Wide w = new CppTypes.Wide();
        w.i8 = -5;
        w.wc = 'é';
        w.ws = "wide é世 😀";
        w.bws = "abc";
        w.ld = -1.0 / 3;
        w.bools = new boolean[] {true, false, true};
        w.bool_array = new boolean[][] {{true, false}, {false, true}};
        w.wchars = new char[] {'a', '世'};
        w.grid = new int[][] {{1, 2, 3}, {4, 5, 6}};
        w.rows = new int[][] {{7, 8, 9}, {10, 11, 12}};
        w.row_seq = new int[][] {{1, 1, 1}, {2, 2, 2}};
        w.strings = new String[] {"x", "yy"};
        w.nested = new String[][] {{"a", "b"}, {}, {"c"}};
        w.level = CppTypes.Level.LOW;
        w.maybe = new CppTypes.ByChar();
        w.maybe.bc('c', "bee");
        w.by_level.high(new int[][] {{1, 2, 3}, {4, 5, 6}});
        w.by_char.a(-1);
        w.by_bool.yes(2.5);
        CppTypes.ByBool no = new CppTypes.ByBool();
        no.__default();
        w.by_bools = new CppTypes.ByBool[] {w.by_bool, no};
        CppTypes.Wide back = roundTrip(CppTypes.WideCodec.INSTANCE, w);
        assertEquals(-5, back.i8);
        assertEquals('é', back.wc);
        assertEquals(w.ws, back.ws);
        assertEquals(-1.0 / 3, back.ld);
        assertTrue(Arrays.deepEquals(w.bool_array, back.bool_array));
        assertArrayEquals(w.wchars, back.wchars);
        assertTrue(Arrays.deepEquals(w.grid, back.grid));
        assertTrue(Arrays.deepEquals(w.rows, back.rows));
        assertTrue(Arrays.deepEquals(w.row_seq, back.row_seq));
        assertTrue(Arrays.deepEquals(w.nested, back.nested));
        assertEquals(CppTypes.Level.LOW, back.level);
        assertEquals('c', back.maybe.discriminator());
        assertEquals("bee", back.maybe.bc());
        assertEquals(6, back.by_level.high()[1][2]);
        assertEquals(false, back.by_bools[1].discriminator());

        w.by_level.other(CppTypes.Level.MID, "other");
        w.maybe = null;
        w.level = null;
        back = roundTrip(CppTypes.WideCodec.INSTANCE, w);
        assertEquals("other", back.by_level.other());
        assertEquals(CppTypes.Level.MID, back.by_level.discriminator());
        assertNull(back.maybe);
        assertNull(back.level);
        w.by_level.low((short) -7);
        assertEquals(-7, roundTrip(CppTypes.WideCodec.INSTANCE, w).by_level.low());
        roundTrip(CppTypes.WideCodec.INSTANCE, new CppTypes.Wide());
    }

    @Test
    void enumsAndBitmasks() {
        assertEquals(-3, CppTypes.Level.LOW.getValue());
        assertEquals(CppTypes.Level.HIGH, CppTypes.Level.valueOf(40));
        assertThrows(IllegalArgumentException.class, () -> CppTypes.Level.valueOf(1));
        assertEquals(0b100011, Gen.PermCodec.bits(Gen.PermCodec.of(0b100011)));
        BitSet tooBig = new BitSet();
        tooBig.set(8);
        assertThrows(IndexOutOfBoundsException.class, () -> Gen.PermCodec.bits(tooBig));
    }

    @Test
    void binary128() {
        double[] values = {0.0, -0.0, 1.0, -2.5, 1.0 / 3, 1e300, Double.MIN_NORMAL, Double.MIN_VALUE,
            Double.MAX_VALUE, Double.POSITIVE_INFINITY, Double.NEGATIVE_INFINITY};
        for (double v : values) {
            double back = Float128.toDouble(Float128.high(v), Float128.low(v));
            assertEquals(Double.doubleToRawLongBits(v), Double.doubleToRawLongBits(back), "" + v);
        }
        assertEquals(0x3fff000000000000L, Float128.high(1.0));
        assertEquals(0, Float128.low(1.0));
        assertEquals(0xc000000000000000L, Float128.high(-2.0));
        assertTrue(Double.isNaN(Float128.toDouble(Float128.high(Double.NaN), Float128.low(Double.NaN))));
    }

    @Test
    void valuesThatDoNotFit() {
        // A string longer than its bound.
        assertThrows(IllegalArgumentException.class,
                () -> serialize(Gen.InnerCodec.INSTANCE, new Gen.Inner(1, "x".repeat(301)), false));
        // A sequence longer than its bound.
        Gen.All a = all();
        a.ls = new int[11];
        assertThrows(IllegalArgumentException.class,
                () -> serialize(Gen.TopCodec.INSTANCE, new Gen.Top(1, a), true));
        // An array of another length.
        Gen.All b = all();
        b.big_arr = new int[299];
        assertThrows(IllegalArgumentException.class,
                () -> serialize(Gen.TopCodec.INSTANCE, new Gen.Top(1, b), true));
        Gen.All c = all();
        c.m = new int[][] {{1, 2, 3}, {4, 5}};
        assertThrows(IllegalArgumentException.class,
                () -> serialize(Gen.TopCodec.INSTANCE, new Gen.Top(1, c), true));
        // A null string.
        assertThrows(IllegalArgumentException.class,
                () -> serialize(HelloWorld.GreetingCodec.INSTANCE, new HelloWorld.Greeting(1, null), false));
        // A union member its discriminator does not select.
        Gen.U u = new Gen.U();
        assertThrows(IllegalStateException.class, u::discriminator);
        u.x(1);
        assertThrows(IllegalStateException.class, u::s);
        assertThrows(IllegalStateException.class, () -> u.x(3, 1));
        // A char that is not in ISO-8859-1.
        Gen.Plain p = new Gen.Plain();
        p.c = '世';
        assertThrows(IllegalArgumentException.class, () -> serialize(Gen.PlainCodec.INSTANCE, p, false));
    }

    @Test
    void streamErrorsAreReported() {
        // Writing a Greeting's values for a Shape fails.
        try (Out out = new Out()) {
            HelloWorld.GreetingCodec.INSTANCE.write(out, new HelloWorld.Greeting(1, "hi"));
            HelloWorld.GreetingCodec.INSTANCE.getType().serialize(out, false);
            assertThrows(IllegalArgumentException.class, () -> Keyed.ShapeCodec.INSTANCE.getType().serialize(out, false));
        }
        // Reading data cut short fails.
        byte[] shape = serialize(Keyed.ShapeCodec.INSTANCE, new Keyed.Shape(1, 2, "three"), false);
        assertThrows(IllegalArgumentException.class,
                () -> deserialize(Keyed.ShapeCodec.INSTANCE, Arrays.copyOf(shape, shape.length - 3)));
    }
}

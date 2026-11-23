// The DDS type support rustorb-idl writes for C# (generated/): the types it
// builds hash as Cyclone DDS's idlc describes them, and samples go through
// XCDR1 and XCDR2 and back unchanged.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.IO;
using System.Linq;
using Omg.Types;
using Xunit;

namespace Rustorb.Dds.Tests;

public class TypesTests
{
    internal static string Root()
    {
        string? d = AppContext.BaseDirectory;
        while (d != null && !File.Exists(Path.Combine(d, "crates", "rustorb-dds-ffi", "Cargo.toml")))
        {
            d = Path.GetDirectoryName(d);
        }
        return d ?? throw new InvalidOperationException("the rustorb-dds checkout is not found");
    }

    static string Fixtures => Path.Combine(Root(), "crates", "rustorb-xtypes", "tests", "fixtures", "cyclonedds");

    /// <summary>The minimal and complete hashes of a fixture's type, from
    /// idlc's TypeInformation (as cpp/tests/test_types.cc reads them).</summary>
    static string IdlcHashes(string name)
    {
        string text = string.Concat(File.ReadAllText(Path.Combine(Fixtures, name + ".typeinfo")).Where(c => !char.IsWhiteSpace(c)));
        if (text.Length % 2 != 0)
        {
            text = text[..^1];
        }
        byte[] data = Convert.FromHexString(text);
        int i = Find(data, new byte[] { 0xf1 }, 0);
        int j = Find(data, new byte[] { 0xf2 }, Find(data, new byte[] { 0x02, 0x10, 0x00, 0x40 }, 0));
        return Hex(data, i + 1) + " " + Hex(data, j + 1);
    }

    static int Find(byte[] data, byte[] pattern, int from)
    {
        for (int i = from; i + pattern.Length <= data.Length; i++)
        {
            if (data.AsSpan(i, pattern.Length).SequenceEqual(pattern))
            {
                return i;
            }
        }
        throw new InvalidOperationException("pattern not found");
    }

    static string Hex(byte[] b, int at) => Convert.ToHexString(b, at, 14).ToLowerInvariant();

    static void CheckHashes<T>(ICodec<T> codec, string fixture)
    {
        (byte[] min, byte[] complete) = codec.Type.Hashes();
        Assert.Equal(IdlcHashes(fixture), Hex(min, 0) + " " + Hex(complete, 0));
    }

    /// <summary>v through XCDR1 and XCDR2 and back: the same encoding again,
    /// and the value read.</summary>
    internal static T RoundTrip<T>(T v)
    {
        T back = default!;
        foreach (bool xcdr2 in new[] { false, true })
        {
            byte[] data = TypeSupport.Serialize(v, xcdr2);
            back = TypeSupport.Deserialize<T>(data);
            Assert.Equal(data, TypeSupport.Serialize(back, xcdr2));
        }
        return back;
    }

    static Gen.Inner Inner(int id) => new(id, new string('x', id + 1));

    static Gen.All All()
    {
        var a = new Gen.All
        {
            color = Gen.Color.BLUE,
            perm = Gen.Perm.READ | Gen.Perm.EXEC,
            name = "name",
            m = new[] { new[] { 1, 2, 3 }, new[] { 4, 5, 6 } },
            arr = new[] { Inner(4), Inner(5), Inner(6), Inner(7) },
            ext = Inner(9),
            opt = 42,
            h = new Gen.Hashed(1, "two", 3),
            p = new Gen.Plain(true, 0xfe, -2, 3, -4, 5, -6, 7, 1.5f, -2.25, 'z'),
        };
        a.ls.AddRange(new[] { 1, 2, 3 });
        a.inners.AddRange(new[] { Inner(1), Inner(2) });
        a.ss.Add(new Sequence<int> { 1 });
        a.ss.Add(new Sequence<int>());
        a.ss.Add(new Sequence<int> { 2, 3 });
        for (int i = 0; i < 300; i++)
        {
            a.big_arr[i] = i * 7;
        }
        a.strs.AddRange(new[] { "a", "", "ccc" });
        a.u.s = "three";
        a.ue.n = "blue";
        a.mut.a = 5;
        a.mut.b = "bee";
        a.mut.k = 9;
        a.mut.inner = Inner(8);
        return a;
    }

    [Fact]
    public void HashesLikeIdlc()
    {
        CheckHashes(Gen.PlainCodec.Instance, "Gen_Plain");
        CheckHashes(Gen.InnerCodec.Instance, "Gen_Inner");
        CheckHashes(Gen.MutCodec.Instance, "Gen_Mut");
        CheckHashes(Gen.UCodec.Instance, "Gen_U");
        CheckHashes(Gen.UECodec.Instance, "Gen_UE");
        CheckHashes(Gen.HashedCodec.Instance, "Gen_Hashed");
        CheckHashes(Gen.AllCodec.Instance, "Gen_All");
        CheckHashes(Gen.TopCodec.Instance, "Gen_Top");
        CheckHashes(HelloWorld.GreetingCodec.Instance, "HelloWorld_Greeting");
        CheckHashes(Keyed.ShapeCodec.Instance, "Keyed_Shape");
    }

    [Fact]
    public void RoundTripEverything()
    {
        Gen.All orig = All();
        Gen.Top top = RoundTrip(new Gen.Top(7, orig));
        Assert.Equal(7, top.id);
        Assert.Equal(orig, top.all);
        Gen.All a = top.all;
        Assert.Equal(Gen.Color.BLUE, a.color);
        Assert.Equal(Gen.Perm.READ | Gen.Perm.EXEC, a.perm);
        Assert.Equal(new[] { 1, 2, 3 }, a.ls);
        Assert.Equal(6, a.m[1][2]);
        Assert.Equal("xxx", a.inners[1].big);
        Assert.Equal(new[] { 2, 3 }, a.ss[2]);
        Assert.Equal(7 * 299, a.big_arr[299]);
        Assert.Equal("three", a.u.s);
        Assert.Equal(3, a.u.Discriminator);
        Assert.Equal("blue", a.ue.n);
        Assert.Equal("bee", a.mut.b);
        Assert.Equal("two", a.h.beta);
        Assert.Equal('z', a.p.c);
        Assert.Equal(0xfe, a.p.o);
        Assert.Equal(42, a.opt);

        // The other members of the unions, and absent optionals.
        Gen.All b = All();
        b.u.Setx(5, 2);
        b.ue.r = 0.5;
        b.opt = null;
        b.mut.b = null;
        Gen.All back = RoundTrip(new Gen.Top(1, b)).all;
        Assert.Equal(2, back.u.Discriminator);
        Assert.Equal(5, back.u.x);
        Assert.Equal(0.5, back.ue.r);
        Assert.Null(back.opt);
        Assert.Null(back.mut.b);
        b.u.Seti(Inner(3), -4);
        back = RoundTrip(new Gen.Top(2, b)).all;
        Assert.Equal(-4, back.u.Discriminator);
        Assert.Equal(3, back.u.i.id);
        Assert.Equal(b, back);

        // The default value, and the codec's.
        RoundTrip(new Gen.Top());
        RoundTrip(Gen.TopCodec.Instance.NewValue());
        Assert.Equal("Gen::Top", Gen.TopCodec.Instance.TypeName);
        Assert.Same(Gen.TopCodec.Instance, TypeSupport.Get<Gen.Top>());
    }

    [Fact]
    public void RoundTripWideAndMore()
    {
        var w = new CppTypes.Wide
        {
            i8 = -5,
            wc = 'é',
            ws = "wide é世 😀",
            bws = "abc",
            ld = -1m / 3,
            bool_array = new[] { new[] { true, false }, new[] { false, true } },
            grid = new[] { new[] { 1, 2, 3 }, new[] { 4, 5, 6 } },
            rows = new[] { new[] { 7, 8, 9 }, new[] { 10, 11, 12 } },
            level = CppTypes.Level.LOW,
            maybe = new CppTypes.ByChar(),
        };
        w.bools.AddRange(new[] { true, false, true });
        w.wchars.AddRange(new[] { 'a', '世' });
        w.row_seq.Add(new[] { 1, 1, 1 });
        w.row_seq.Add(new[] { 2, 2, 2 });
        w.strings.AddRange(new[] { "x", "yy" });
        w.nested.Add(new Sequence<string> { "a", "b" });
        w.nested.Add(new Sequence<string>());
        w.nested.Add(new Sequence<string> { "c" });
        w.maybe.Setbc("bee", 'c');
        w.by_level.high = new[] { new[] { 1, 2, 3 }, new[] { 4, 5, 6 } };
        w.by_char.a = -1;
        w.by_bool.yes = 2.5;
        var no = new CppTypes.ByBool();
        no._default();
        w.by_bools.Add(w.by_bool);
        w.by_bools.Add(no);
        CppTypes.Wide back = RoundTrip(w);
        Assert.Equal(w, back);
        Assert.Equal(-5, back.i8);
        Assert.Equal('é', back.wc);
        Assert.Equal(w.ws, back.ws);
        Assert.Equal(w.ld, back.ld);
        Assert.Equal(CppTypes.Level.LOW, back.level);
        Assert.Equal('c', back.maybe!.Discriminator);
        Assert.Equal("bee", back.maybe.bc);
        Assert.Equal(6, back.by_level.high[1][2]);
        Assert.False(back.by_bools[1].Discriminator);

        w.by_level.Setother("other", CppTypes.Level.MID);
        w.maybe = null;
        w.level = null;
        back = RoundTrip(w);
        Assert.Equal("other", back.by_level.other);
        Assert.Equal(CppTypes.Level.MID, back.by_level.Discriminator);
        Assert.Null(back.maybe);
        Assert.Null(back.level);
        w.by_level.low = -7;
        Assert.Equal(-7, RoundTrip(w).by_level.low);
        RoundTrip(new CppTypes.Wide());
    }

    [Fact]
    public void Binary128()
    {
        decimal[] values = { 0m, 1m, -2.5m, 1m / 3, decimal.MaxValue, decimal.MinValue, 0.0000000001m, 123456789.987654321m };
        foreach (decimal v in values)
        {
            (ulong hi, ulong lo) = Float128.FromDecimal(v);
            Assert.Equal(v, Float128.ToDecimal(hi, lo));
        }
        Assert.Equal((0x3fff000000000000UL, 0UL), Float128.FromDecimal(1m));
        Assert.Equal((0xc000000000000000UL, 0UL), Float128.FromDecimal(-2m));
    }

    [Fact]
    public void ValuesThatDoNotFit()
    {
        // A string longer than its bound.
        Assert.ThrowsAny<ArgumentException>(() => TypeSupport.Serialize(new Gen.Inner(1, new string('x', 301))));
        // A sequence longer than its bound.
        Gen.All a = All();
        a.ls.AddRange(new int[8]);
        Assert.ThrowsAny<ArgumentException>(() => TypeSupport.Serialize(new Gen.Top(1, a), true));
        // An array of another length.
        Gen.All b = All();
        b.big_arr = new int[299];
        Assert.ThrowsAny<ArgumentException>(() => TypeSupport.Serialize(new Gen.Top(1, b), true));
        Gen.All c = All();
        c.m = new[] { new[] { 1, 2, 3 }, new[] { 4, 5 } };
        Assert.ThrowsAny<ArgumentException>(() => TypeSupport.Serialize(new Gen.Top(1, c), true));
        // A null string.
        Assert.ThrowsAny<ArgumentException>(() => TypeSupport.Serialize(new HelloWorld.Greeting(1, null!)));
        // A union member its discriminator does not select.
        var u = new Gen.U { x = 1 };
        Assert.Throws<InvalidOperationException>(() => u.s);
        Assert.Throws<ArgumentException>(() => u.Setx(1, 3));
        // A char that is not in ISO-8859-1.
        var p = new Gen.Plain { c = '世' };
        Assert.ThrowsAny<ArgumentException>(() => TypeSupport.Serialize(p));
    }

    [Fact]
    public void StreamErrorsAreReported()
    {
        // Reading data cut short fails.
        byte[] shape = TypeSupport.Serialize(new Keyed.Shape(1, 2, "three"));
        Assert.ThrowsAny<ArgumentException>(() => TypeSupport.Deserialize<Keyed.Shape>(shape[..^3]));
    }

    [Fact]
    public void ValuesHelpers()
    {
        Assert.True(Values.Equal(new[] { new[] { 1 } }, new Sequence<int[]> { new[] { 1 } }));
        Assert.False(Values.Equal(new[] { 1, 2 }, new[] { 1 }));
        Assert.Equal(Values.Hash(new[] { new[] { 1, 2 } }), Values.Hash(new Sequence<int[]> { new[] { 1, 2 } }));
        Assert.Equal(new[] { 3, 3 }, Values.Array(2, () => 3));
    }
}

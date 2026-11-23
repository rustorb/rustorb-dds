# frozen_string_literal: true

# The DDS type support rustorb-idl writes for Ruby (generated/): the types
# it builds hash as Cyclone DDS's idlc describes them, and samples go
# through XCDR1 and XCDR2 and back unchanged.
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

require_relative "helper"

class TestTypes < Minitest::Test
  FIXTURES = File.join(TestHelper::ROOT, "crates", "rustorb-xtypes", "tests", "fixtures", "cyclonedds")

  # The minimal and complete hashes of a fixture's type, from idlc's
  # TypeInformation (as cpp/tests/test_types.cc reads them).
  def idlc_hashes(name)
    text = File.read(File.join(FIXTURES, "#{name}.typeinfo")).gsub(/\s/, "")
    text = text[0...-1] if text.size.odd?
    data = [text].pack("H*")
    i = data.index("\xf1".b)
    j = data.index("\xf2".b, data.index("\x02\x10\x00\x40".b))
    [data[i + 1, 14], data[j + 1, 14]].map { |h| h.unpack1("H*") }
  end

  def check_hashes(klass, fixture)
    assert_equal idlc_hashes(fixture), DDS::Types.of(klass).hashes.map { |h| h.unpack1("H*") }, fixture
  end

  # v through XCDR1 and XCDR2 and back: the same encoding again, and the
  # value read.
  def round_trip(v)
    back = nil
    [false, true].each do |xcdr2|
      data = DDS::Types.serialize(v, xcdr2: xcdr2)
      back = DDS::Types.deserialize(v.class, data)
      assert_equal data, DDS::Types.serialize(back, xcdr2: xcdr2)
    end
    back
  end

  def inner(id)
    Gen::Inner.new(id, "x" * (id + 1))
  end

  def all
    a = Gen::All.new
    a.color = Gen::Color::BLUE
    a.perm = Gen::Perm::READ | Gen::Perm::EXEC
    a.name = "name"
    a.m = [[1, 2, 3], [4, 5, 6]]
    a.arr = [inner(4), inner(5), inner(6), inner(7)]
    a.ext = inner(9)
    a.opt = 42
    a.h = Gen::Hashed.new(1, "two", 3)
    a.p = Gen::Plain.new(true, 0xfe, -2, 3, -4, 5, -6, 7, 1.5, -2.25, "z")
    a.ls = [1, 2, 3]
    a.inners = [inner(1), inner(2)]
    a.ss = [[1], [], [2, 3]]
    a.big_arr = Array.new(300) { |i| i * 7 }
    a.strs = ["a", "", "ccc"]
    a.u.s = "three"
    a.ue.n = "blue"
    a.mut.a = 5
    a.mut.b = "bee"
    a.mut.k = 9
    a.mut.inner = inner(8)
    a
  end

  def test_hashes_like_idlc
    check_hashes(Gen::Plain, "Gen_Plain")
    check_hashes(Gen::Inner, "Gen_Inner")
    check_hashes(Gen::Mut, "Gen_Mut")
    check_hashes(Gen::U, "Gen_U")
    check_hashes(Gen::UE, "Gen_UE")
    check_hashes(Gen::Hashed, "Gen_Hashed")
    check_hashes(Gen::All, "Gen_All")
    check_hashes(Gen::Top, "Gen_Top")
    check_hashes(HelloWorld::Greeting, "HelloWorld_Greeting")
    check_hashes(Keyed::Shape, "Keyed_Shape")
  end

  def test_round_trip_everything
    orig = all
    top = round_trip(Gen::Top.new(7, orig))
    assert_equal 7, top.id
    assert_equal orig, top.all
    a = top.all
    assert_equal Gen::Color::BLUE, a.color
    assert_equal Gen::Perm::READ | Gen::Perm::EXEC, a.perm
    assert_equal [1, 2, 3], a.ls
    assert_equal 6, a.m[1][2]
    assert_equal "xxx", a.inners[1].big
    assert_equal [2, 3], a.ss[2]
    assert_equal 7 * 299, a.big_arr[299]
    assert_equal "three", a.u.s
    assert_equal 3, a.u._disc
    assert_equal "blue", a.ue.n
    assert_equal "bee", a.mut.b
    assert_equal "two", a.h.beta
    assert_equal "z", a.p.c
    assert_equal 0xfe, a.p.o
    assert_equal 42, a.opt

    # The other members of the unions, and absent optionals.
    b = all
    b.u._put("x", 5, 2)
    b.ue.r = 0.5
    b.opt = nil
    b.mut.b = nil
    back = round_trip(Gen::Top.new(1, b)).all
    assert_equal 2, back.u._disc
    assert_equal 5, back.u.x
    assert_equal 0.5, back.ue.r
    assert_nil back.opt
    assert_nil back.mut.b
    b.u._put("i", inner(3), -4)
    back = round_trip(Gen::Top.new(2, b)).all
    assert_equal(-4, back.u._disc)
    assert_equal 3, back.u.i.id
    assert_equal b, back

    # New values.
    round_trip(Gen::Top.new)
    assert_equal "Gen::Top", DDS::Types.of(Gen::Top).name
  end

  def test_round_trip_wide_and_more
    w = CppTypes::Wide.new
    w.i8 = -5
    w.wc = "é".ord
    w.ws = "wide é世 😀"
    w.bws = "abc".encode(Encoding::UTF_16LE).unpack("v*")
    w.ld = DDS::LongDouble.new(-1.0 / 3)
    w.bool_array = [[true, false], [false, true]]
    w.grid = [[1, 2, 3], [4, 5, 6]]
    w.rows = [[7, 8, 9], [10, 11, 12]]
    w.level = CppTypes::Level::LOW
    w.maybe = CppTypes::ByChar.new
    w.bools = [true, false, true]
    w.wchars = ["a".ord, "世".ord]
    w.row_seq = [[1, 1, 1], [2, 2, 2]]
    w.strings = %w[x yy]
    w.nested = [%w[a b], [], ["c"]]
    w.maybe._put("bc", "bee", "c")
    w.by_level.high = [[1, 2, 3], [4, 5, 6]]
    w.by_char.a = -1
    w.by_bool.yes = 2.5
    no = CppTypes::ByBool.new
    no._default
    w.by_bools = [w.by_bool, no]
    back = round_trip(w)
    # A wstring written as a String comes back as its UTF-16 code units.
    assert_equal "wide é世 😀".encode(Encoding::UTF_16LE).unpack("v*"), back.ws
    w.ws = back.ws
    assert_equal w, back
    assert_equal(-5, back.i8)
    assert_equal "é".ord, back.wc
    assert_equal(-1.0 / 3, back.ld.to_f)
    assert_equal CppTypes::Level::LOW, back.level
    assert_equal "c", back.maybe._disc
    assert_equal "bee", back.maybe.bc
    assert_equal 6, back.by_level.high[1][2]
    assert_equal false, back.by_bools[1]._disc
    assert_nil back.by_bools[1]._member

    w.by_level._put("other", [0x41], CppTypes::Level::MID)
    w.maybe = nil
    w.level = nil
    back = round_trip(w)
    assert_equal [0x41], back.by_level.other
    assert_equal CppTypes::Level::MID, back.by_level._disc
    assert_nil back.maybe
    assert_nil back.level
    w.by_level.low = -7
    assert_equal(-7, round_trip(w).by_level.low)
    round_trip(CppTypes::Wide.new)
  end

  def test_binary128
    [0.0, 1.0, -2.5, 1.0 / 3, Float::MAX, -Float::MAX, Float::MIN, 5e-324, 1e-10, 123_456_789.987654321].each do |v|
      assert_equal v, DDS::LongDouble.new(v).to_f
    end
    assert_equal [0x3fff000000000000, 0], DDS::LongDouble.bits_of(1.0)
    assert_equal [0xc000000000000000, 0], DDS::LongDouble.bits_of(-2.0)
    assert DDS::LongDouble.new(Float::INFINITY).to_f.infinite?
    assert DDS::LongDouble.new(Float::NAN).to_f.nan?
    assert_equal DDS::LongDouble.new(0.5), 0.5
  end

  def test_values_that_do_not_fit
    bad = DDS::Core::InvalidArgumentError
    # A string longer than its bound.
    assert_raises(bad) { DDS::Types.serialize(Gen::Inner.new(1, "x" * 301)) }
    # A sequence longer than its bound.
    a = all
    a.ls += [0] * 8
    assert_raises(bad) { DDS::Types.serialize(Gen::Top.new(1, a), xcdr2: true) }
    # An array of another length.
    b = all
    b.big_arr = [0] * 299
    assert_raises(bad) { DDS::Types.serialize(Gen::Top.new(1, b), xcdr2: true) }
    c = all
    c.m = [[1, 2, 3], [4, 5]]
    assert_raises(bad) { DDS::Types.serialize(Gen::Top.new(1, c), xcdr2: true) }
    # A nil string, a value of the wrong type, an integer out of range.
    assert_raises(bad) { DDS::Types.serialize(HelloWorld::Greeting.new(1, nil)) }
    assert_raises(bad) { DDS::Types.serialize(HelloWorld::Greeting.new("1", "x")) }
    assert_raises(bad) { DDS::Types.serialize(HelloWorld::Greeting.new(-1, "x")) }
    assert_raises(bad) { DDS::Types.serialize(HelloWorld::Greeting.new(2**32, "x")) }
    assert_raises(bad) { DDS::Types.serialize(HelloWorld::Greeting.new(2**70, "x")) }
    # An enum value that is not one, bits outside a bitmask.
    d = all
    d.color = 3
    assert_raises(bad) { DDS::Types.serialize(Gen::Top.new(1, d)) }
    d = all
    d.perm = 1 << 8
    assert_raises(bad) { DDS::Types.serialize(Gen::Top.new(1, d)) }
    # A union member its discriminator does not select.
    u = Gen::U.new
    u.x = 1
    assert_raises(DDS::Core::PreconditionNotMetError) { u.s }
    assert_raises(bad) { u._put("x", 1, 3) }
    # A char that is not in ISO-8859-1.
    p = Gen::Plain.new
    p.c = "世"
    assert_raises(bad) { DDS::Types.serialize(p) }
  end

  def test_stream_errors_are_reported
    shape = DDS::Types.serialize(Keyed::Shape.new(1, 2, "three"))
    assert_raises(DDS::Core::Error) { DDS::Types.deserialize(Keyed::Shape, shape[0...-3]) }
  end

  def test_new_values
    g = HelloWorld::Greeting.new
    assert_equal 0, g.index
    assert_equal "", g.message
    assert_equal HelloWorld::Greeting.new(1, "a"), HelloWorld::Greeting.new(1, "a")
    refute_equal HelloWorld::Greeting.new(1, "a"), HelloWorld::Greeting.new(1, "b")
    assert_raises(ArgumentError) { HelloWorld::Greeting.new(1, "a", 2) }
    # The default member of a new union, with its zero value.
    u = Gen::U.new
    assert_equal "i", u._member
    assert_equal Gen::Inner.new, u.i
    assert_nil Gen::UE.new._member
  end
end

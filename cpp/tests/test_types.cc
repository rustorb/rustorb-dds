// The DDS type support rustorb-idl writes for C++ (tests/generated): the
// types it builds hash as Cyclone DDS's idlc describes them, and samples go
// through XCDR1 and XCDR2 and back unchanged.

#include "CppTypes_dds.hh"
#include "Generated_dds.hh"
#include "HelloWorld_dds.hh"
#include "Keyed_dds.hh"

#include <dds/dds.hpp>

#include <algorithm>
#include <cmath>
#include <fstream>
#include <limits>
#include <sstream>

#include "check.hpp"

namespace {

std::string hex(const uint8_t* p, size_t n) {
  static const char d[] = "0123456789abcdef";
  std::string s;
  for (size_t i = 0; i < n; i++) {
    s += d[p[i] >> 4];
    s += d[p[i] & 15];
  }
  return s;
}

// The minimal and complete hashes of a fixture's type, from idlc's
// TypeInformation (see python/tests/test_generated.py).
std::pair<std::string, std::string> idlc_hashes(const std::string& name) {
  std::ifstream f(std::string(RUSTORB_DDS_FIXTURES) + "/" + name + ".typeinfo");
  CHECK(f.good());
  std::string text, word;
  while (f >> word) text += word;
  std::vector<uint8_t> data;
  for (size_t i = 0; i + 1 < text.size(); i += 2)
    data.push_back(static_cast<uint8_t>(std::stoi(text.substr(i, 2), nullptr, 16)));
  auto find = [&](const std::vector<uint8_t>& pat, size_t from) {
    for (size_t i = from; i + pat.size() <= data.size(); i++)
      if (std::equal(pat.begin(), pat.end(), data.begin() + static_cast<std::ptrdiff_t>(i))) return i;
    throw check::failure{"pattern not found in " + name};
  };
  size_t i = find({0xf1}, 0);
  size_t j = find({0xf2}, find({0x02, 0x10, 0x00, 0x40}, 0));
  return {hex(&data[i + 1], 14), hex(&data[j + 1], 14)};
}

template <typename T>
void check_hashes(const std::string& fixture) {
  uint8_t minimal[14], complete[14];
  rustorb::dds::detail::check(rdds_type_hashes(rustorb::dds::codec<T>::type(), minimal, complete));
  auto expected = idlc_hashes(fixture);
  if (hex(minimal, 14) != expected.first || hex(complete, 14) != expected.second)
    throw check::failure{fixture + ": " + hex(minimal, 14) + " " + hex(complete, 14) + " != " + expected.first +
                         " " + expected.second};
}

// v through XCDR (1, then 2) and back.
template <typename T>
T round_trip(const T& v, bool xcdr2) {
  rustorb::dds::out_stream s;
  rustorb::dds::write_sample(s, v);
  uint8_t* data = nullptr;
  size_t len = 0;
  rustorb::dds::detail::check(rdds_type_serialize(rustorb::dds::codec<T>::type(), s.get(), xcdr2, &data, &len));
  rdds_in* in = nullptr;
  rdds_status st = rdds_type_deserialize(rustorb::dds::codec<T>::type(), data, len, &in);
  rdds_bytes_free(data, len);
  rustorb::dds::detail::check(st);
  rustorb::dds::in_stream is(in);
  T back;
  rustorb::dds::read_sample(is.get(), back);
  CHECK(rdds_in_at_end(is.get()));
  return back;
}

template <typename T>
void check_round_trip(const T& v) {
  CHECK(round_trip(v, false) == v);
  CHECK(round_trip(v, true) == v);
}

Gen::Inner inner(int32_t id) { return Gen::Inner(id, std::string(static_cast<size_t>(id) + 1, 'x')); }

Gen::All all() {
  Gen::All a;
  a.color(Gen::Color::BLUE);
  a.perm(Gen::Perm::READ | Gen::Perm::EXEC);
  a.ls({1, 2, 3});
  a.name("name");
  a.m({{{{1, 2, 3}}, {{4, 5, 6}}}});
  a.inners({inner(1), inner(2)});
  a.ss({{1}, {}, {2, 3}});
  a.arr({{inner(4), inner(5), inner(6), inner(7)}});
  for (size_t i = 0; i < 300; i++) a.big_arr()[i] = static_cast<int32_t>(i * 7);
  a.strs({"a", "", "ccc"});
  Gen::U u;
  u.s("three");
  a.u(u);
  Gen::UE ue;
  ue.n("blue");
  a.ue(ue);
  Gen::Mut mut;
  mut.a(5);
  mut.b(std::string("bee"));
  mut.k(9);
  mut.inner(inner(8));
  a.mut(mut);
  a.h(Gen::Hashed(1, "two", 3));
  a.p(Gen::Plain(true, 0xfe, -2, 3, -4, 5, -6, 7, 1.5f, -2.25, 'z'));
  a.ext(inner(9));
  a.opt(42);
  return a;
}

}  // namespace

TEST(hashes_like_idlc) {
  check_hashes<Gen::Plain>("Gen_Plain");
  check_hashes<Gen::Inner>("Gen_Inner");
  check_hashes<Gen::Mut>("Gen_Mut");
  check_hashes<Gen::U>("Gen_U");
  check_hashes<Gen::UE>("Gen_UE");
  check_hashes<Gen::Hashed>("Gen_Hashed");
  check_hashes<Gen::All>("Gen_All");
  check_hashes<Gen::Top>("Gen_Top");
  check_hashes<HelloWorld::Greeting>("HelloWorld_Greeting");
  check_hashes<Keyed::Shape>("Keyed_Shape");
}

TEST(topic_traits) {
  CHECK(dds::topic::is_topic_type<Gen::Top>::value);
  CHECK(dds::topic::is_topic_type<HelloWorld::Greeting>::value);
  CHECK(dds::topic::topic_type_name<Gen::Top>::value() == "Gen::Top");
  CHECK(!dds::topic::is_topic_type<int>::value);
}

TEST(round_trip_everything) {
  Gen::Top top(7, all());
  check_round_trip(top);
  // the other members of the unions, and an absent optional
  Gen::All a = all();
  Gen::U u;
  u.x(2);
  a.u(u);
  u.i(inner(3));
  Gen::All b = all();
  b.u(u);
  Gen::UE ue;
  ue.r(0.5);
  b.ue(ue);
  b.opt(std::nullopt);
  Gen::Mut m = b.mut();
  m.b(std::nullopt);
  b.mut(m);
  check_round_trip(Gen::Top(1, a));
  check_round_trip(Gen::Top(2, b));
  // the default sample
  check_round_trip(Gen::Top());
}

TEST(round_trip_wide_and_more) {
  CppTypes::Wide w;
  w.i8(-5);
  w.wc(L'é');
  w.ws(L"wide é世 \U0001F600");
  w.bws(L"abc");
  w.ld(-1.0L / 3);
  w.bools({true, false, true});
  w.bool_array({{{{true, false}}, {{false, true}}}});
  w.wchars({L'a', L'世'});
  w.grid({{{{1, 2, 3}}, {{4, 5, 6}}}});
  w.rows({{{{7, 8, 9}}, {{10, 11, 12}}}});
  w.row_seq({{{1, 1, 1}}, {{2, 2, 2}}});
  w.strings({"x", "yy"});
  w.nested({{"a", "b"}, {}, {"c"}});
  w.level(CppTypes::Level::LOW);
  CppTypes::ByChar c;
  c.bc(std::string("bee"), 'c');
  w.maybe(c);
  CppTypes::ByLevel bl;
  bl.high({{{{1, 2, 3}}, {{4, 5, 6}}}});
  w.by_level(bl);
  CppTypes::ByChar c2;
  c2.a(-1);
  w.by_char(c2);
  CppTypes::ByBool b;
  b.yes(2.5);
  w.by_bool(b);
  w.by_bools({b, CppTypes::ByBool()});
  check_round_trip(w);

  CppTypes::ByLevel other;
  other.other(L"other");
  w.by_level(other);
  w.maybe(std::nullopt);
  w.level(std::nullopt);
  check_round_trip(w);
  CppTypes::ByLevel low;
  low.low(-7);
  w.by_level(low);
  check_round_trip(w);
  check_round_trip(CppTypes::Wide());
}

TEST(long_double_as_binary128) {
  const long double values[] = {0.0L,
                                -0.0L,
                                1.0L,
                                -2.5L,
                                1.0L / 3,
                                1e300L,
                                std::numeric_limits<long double>::min(),
                                std::numeric_limits<long double>::denorm_min(),
                                std::numeric_limits<long double>::max(),
                                std::numeric_limits<long double>::infinity(),
                                -std::numeric_limits<long double>::infinity()};
  for (long double v : values) {
    uint64_t high = 0, low = 0;
    rustorb::dds::detail::to_binary128(v, high, low);
    long double back = rustorb::dds::detail::from_binary128(high, low);
    CHECK(back == v);
    CHECK(std::signbit(back) == std::signbit(v));
  }
  uint64_t high = 0, low = 0;
  rustorb::dds::detail::to_binary128(1.0L, high, low);
  CHECK(high == 0x3fff000000000000ull && low == 0);
  rustorb::dds::detail::to_binary128(-2.0L, high, low);
  CHECK(high == 0xc000000000000000ull && low == 0);
  rustorb::dds::detail::to_binary128(std::numeric_limits<long double>::quiet_NaN(), high, low);
  CHECK(std::isnan(rustorb::dds::detail::from_binary128(high, low)));
}

TEST(values_that_do_not_fit) {
  // A string longer than its bound.
  Gen::Inner i(1, std::string(301, 'x'));
  CHECK_THROWS(round_trip(i, false), dds::core::InvalidArgumentError);
  // A sequence longer than its bound.
  Gen::All a = all();
  a.ls(std::vector<int32_t>(11, 1));
  CHECK_THROWS(round_trip(a, true), dds::core::InvalidArgumentError);
  // A union member its discriminator does not select.
  Gen::U u;
  u.x(1);
  CHECK_THROWS(u.s(), std::invalid_argument);
  CHECK_THROWS(u.x(1, 3), std::invalid_argument);
}

TEST(stream_errors_are_reported) {
  rustorb::dds::out_stream s;
  rustorb::dds::write_sample(s, HelloWorld::Greeting(1, "hi"));
  uint8_t* data = nullptr;
  size_t len = 0;
  rustorb::dds::detail::check(
      rdds_type_serialize(rustorb::dds::codec<HelloWorld::Greeting>::type(), s.get(), false, &data, &len));
  rdds_bytes_free(data, len);
  // Writing a Greeting's values for a Shape fails.
  rdds_status st =
      rdds_type_serialize(rustorb::dds::codec<Keyed::Shape>::type(), s.get(), false, &data, &len);
  CHECK(st == RDDS_RETCODE_BAD_PARAMETER);
}

int main() { return check::run_all(); }

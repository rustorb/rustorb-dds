// rustorb/dds/types.hpp: what the code rustorb-idl writes for DDS
// (rustorb-idl -bc++11 -Wbdds or -Wbdds_only, Foo_dds.hh) builds on: the
// type builders, the value codecs and the topic type traits.
//
// For each struct, union, enum and bitmask of an IDL file, Foo_dds.hh
// specializes rustorb::dds::codec<T> with
//
//   static const rdds_type* type();          // its DDS-XTypes type
//   static void write(rdds_out*, const T&);  // its values, in type order
//   static void read(rdds_in*, T&);
//
// and, for a topic type (a struct that is not @nested),
// dds::topic::is_topic_type<T> and dds::topic::topic_type_name<T>. The
// values of members go through the shapes of rustorb::dds::shape, which
// say how the IDL type of a member is written (a sequence, an array of
// how many dimensions...), since the C++ type alone does not say it.
//
// SPDX-License-Identifier: MIT OR Apache-2.0
#ifndef RUSTORB_DDS_TYPES_HPP
#define RUSTORB_DDS_TYPES_HPP

#include <rustorb/dds/core.hpp>

#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <initializer_list>
#include <limits>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <type_traits>
#include <tuple>
#include <utility>
#include <vector>

namespace dds {
namespace topic {

// Topic type traits (DDS-PSM-Cxx 1.0, 7.5.10): specialized by the
// generated code for each topic type.
template <typename T>
struct is_topic_type {
  enum { value = 0 };
};

template <typename T>
struct topic_type_name;

}  // namespace topic
}  // namespace dds

namespace rustorb {
namespace dds {

// The version of what the generated code expects of this header.
inline constexpr int generated_code_version = 1;

// ---------------------------------------------------------------------------
// Types

// An rdds_type, released at the end.
class type_ref {
 public:
  type_ref() = default;
  explicit type_ref(rdds_type* p) : p_(p) {}
  type_ref(const type_ref&) = delete;
  type_ref& operator=(const type_ref&) = delete;
  type_ref(type_ref&& o) noexcept : p_(o.p_) { o.p_ = nullptr; }
  type_ref& operator=(type_ref&& o) noexcept {
    std::swap(p_, o.p_);
    return *this;
  }
  ~type_ref() { rdds_type_release(p_); }
  const rdds_type* get() const { return p_; }

 private:
  rdds_type* p_ = nullptr;
};

inline type_ref primitive_type(uint8_t kind) {
  rdds_type* t = nullptr;
  detail::check(rdds_type_primitive(kind, &t));
  return type_ref(t);
}

inline type_ref string_type(bool wide, uint32_t bound) {
  rdds_type* t = nullptr;
  detail::check(rdds_type_string(wide, bound, &t));
  return type_ref(t);
}

inline type_ref sequence_type(const rdds_type* element, uint32_t bound) {
  rdds_type* t = nullptr;
  detail::check(rdds_type_sequence(element, bound, &t));
  return type_ref(t);
}

inline type_ref array_type(const rdds_type* element, std::initializer_list<uint32_t> dims) {
  rdds_type* t = nullptr;
  detail::check(rdds_type_array(element, dims.begin(), dims.size(), &t));
  return type_ref(t);
}

inline type_ref map_type(const rdds_type* key, const rdds_type* element, uint32_t bound) {
  rdds_type* t = nullptr;
  detail::check(rdds_type_map(key, element, bound, &t));
  return type_ref(t);
}

inline type_ref alias_type(const char* name, const rdds_type* related) {
  rdds_type* t = nullptr;
  detail::check(rdds_type_alias(name, related, &t));
  return type_ref(t);
}

class enum_builder {
 public:
  enum_builder(const char* name, uint16_t bit_bound, int32_t extensibility) {
    detail::check(rdds_enum_builder_new(name, bit_bound, extensibility, &b_));
  }
  ~enum_builder() { rdds_enum_builder_release(b_); }
  enum_builder(const enum_builder&) = delete;
  enum_builder& operator=(const enum_builder&) = delete;
  enum_builder& literal(const char* name, int32_t value, bool is_default = false) {
    detail::check(rdds_enum_builder_add_literal(b_, name, value, is_default));
    return *this;
  }
  type_ref build() const {
    rdds_type* t = nullptr;
    detail::check(rdds_enum_builder_build(b_, &t));
    return type_ref(t);
  }

 private:
  rdds_enum_builder* b_ = nullptr;
};

class bitmask_builder {
 public:
  bitmask_builder(const char* name, uint16_t bit_bound, int32_t extensibility) {
    detail::check(rdds_bitmask_builder_new(name, bit_bound, extensibility, &b_));
  }
  ~bitmask_builder() { rdds_bitmask_builder_release(b_); }
  bitmask_builder(const bitmask_builder&) = delete;
  bitmask_builder& operator=(const bitmask_builder&) = delete;
  bitmask_builder& flag(const char* name, uint16_t position) {
    detail::check(rdds_bitmask_builder_add_flag(b_, name, position));
    return *this;
  }
  type_ref build() const {
    rdds_type* t = nullptr;
    detail::check(rdds_bitmask_builder_build(b_, &t));
    return type_ref(t);
  }

 private:
  rdds_bitmask_builder* b_ = nullptr;
};

class struct_builder {
 public:
  struct_builder(const char* name, int32_t extensibility, uint32_t flags,
                 const rdds_type* base = nullptr) {
    detail::check(rdds_struct_builder_new(name, extensibility, flags, base, &b_));
  }
  ~struct_builder() { rdds_struct_builder_release(b_); }
  struct_builder(const struct_builder&) = delete;
  struct_builder& operator=(const struct_builder&) = delete;
  struct_builder& member(const char* name, uint32_t id, const rdds_type* type, uint32_t flags = 0,
                         const char* hash_id = nullptr) {
    detail::check(rdds_struct_builder_add_member(b_, name, id, type, flags, hash_id));
    return *this;
  }
  type_ref build() const {
    rdds_type* t = nullptr;
    detail::check(rdds_struct_builder_build(b_, &t));
    return type_ref(t);
  }

 private:
  rdds_struct_builder* b_ = nullptr;
};

class union_builder {
 public:
  union_builder(const char* name, int32_t extensibility, uint32_t flags,
                const rdds_type* discriminator, uint32_t discriminator_flags = 0) {
    detail::check(rdds_union_builder_new(name, extensibility, flags, discriminator,
                                         discriminator_flags, &b_));
  }
  ~union_builder() { rdds_union_builder_release(b_); }
  union_builder(const union_builder&) = delete;
  union_builder& operator=(const union_builder&) = delete;
  union_builder& member(const char* name, uint32_t id, const rdds_type* type,
                        std::initializer_list<int32_t> labels, uint32_t flags = 0,
                        const char* hash_id = nullptr) {
    detail::check(rdds_union_builder_add_member(b_, name, id, type, labels.begin(), labels.size(),
                                                flags, hash_id));
    return *this;
  }
  type_ref build() const {
    rdds_type* t = nullptr;
    detail::check(rdds_union_builder_build(b_, &t));
    return type_ref(t);
  }

 private:
  rdds_union_builder* b_ = nullptr;
};

// ---------------------------------------------------------------------------
// Values

// The codec of a struct, union, enum or bitmask T: specialized by the
// generated code.
template <typename T>
struct codec;

namespace detail {

// long double <-> the IEEE 754 binary128 of float128: exact for every
// long double format (80-bit x87, binary64, binary128).
inline void to_binary128(long double x, uint64_t& high, uint64_t& low) {
  uint64_t sign = std::signbit(x) ? (uint64_t{1} << 63) : 0;
  long double a = std::fabs(x);
  if (std::isnan(x)) {
    high = sign | (uint64_t{0x7fff} << 48) | (uint64_t{1} << 47);
    low = 0;
    return;
  }
  if (std::isinf(x)) {
    high = sign | (uint64_t{0x7fff} << 48);
    low = 0;
    return;
  }
  if (a == 0) {
    high = sign;
    low = 0;
    return;
  }
  int e = 0;
  long double m = std::frexp(a, &e);  // a = m * 2^e, 0.5 <= m < 1
  int64_t biased = static_cast<int64_t>(e) - 1 + 16383;
  long double f;  // the 112 bits of fraction, as [0, 1)
  if (biased <= 0) {
    f = std::ldexp(a, 16382);  // subnormal: a / 2^-16382
    biased = 0;
  } else {
    f = std::ldexp(m, 1) - 1;
  }
  long double hi = std::floor(std::ldexp(f, 48));
  long double lo = std::ldexp(std::ldexp(f, 48) - hi, 64);
  high = sign | (static_cast<uint64_t>(biased) << 48) | static_cast<uint64_t>(hi);
  low = static_cast<uint64_t>(lo);
}

inline long double from_binary128(uint64_t high, uint64_t low) {
  bool neg = (high >> 63) != 0;
  int exp = static_cast<int>((high >> 48) & 0x7fff);
  uint64_t fhi = high & ((uint64_t{1} << 48) - 1);
  long double r;
  if (exp == 0x7fff) {
    r = (fhi || low) ? std::numeric_limits<long double>::quiet_NaN()
                     : std::numeric_limits<long double>::infinity();
  } else {
    long double f = std::ldexp(static_cast<long double>(fhi), -48) +
                    std::ldexp(static_cast<long double>(low), -112);
    r = exp == 0 ? std::ldexp(f, -16382) : std::ldexp(1 + f, exp - 16383);
  }
  return neg ? -r : r;
}

// wchar_t <-> UTF-16 code units (wchar_t is UTF-32 outside Windows).
inline std::vector<uint16_t> to_utf16(const wchar_t* p, size_t n) {
  std::vector<uint16_t> out;
  out.reserve(n);
  for (size_t i = 0; i < n; i++) {
    uint32_t c = static_cast<uint32_t>(p[i]);
    if (sizeof(wchar_t) > 2 && c > 0xffff) {
      c -= 0x10000;
      out.push_back(static_cast<uint16_t>(0xd800 + (c >> 10)));
      out.push_back(static_cast<uint16_t>(0xdc00 + (c & 0x3ff)));
    } else {
      out.push_back(static_cast<uint16_t>(c));
    }
  }
  return out;
}

inline std::wstring from_utf16(const uint16_t* p, size_t n) {
  std::wstring out;
  out.reserve(n);
  for (size_t i = 0; i < n; i++) {
    uint32_t c = p[i];
    if (sizeof(wchar_t) > 2 && c >= 0xd800 && c < 0xdc00 && i + 1 < n && p[i + 1] >= 0xdc00 &&
        p[i + 1] < 0xe000) {
      c = 0x10000 + ((c - 0xd800) << 10) + (p[i + 1] - 0xdc00);
      i++;
    }
    out.push_back(static_cast<wchar_t>(c));
  }
  return out;
}

}  // namespace detail

namespace shape {

// A primitive: T is bool, uint8_t (octet), int8_t, int16_t, uint16_t,
// int32_t, uint32_t, int64_t, uint64_t, float, double, long double, char or
// wchar_t. bulk: whether sequences and arrays of it go in one call.
template <typename T>
struct prim;

#define RUSTORB_DDS_PRIM(T, NAME)                                                     \
  template <>                                                                         \
  struct prim<T> {                                                                    \
    using value_type = T;                                                             \
    static constexpr bool bulk = true;                                                \
    static void write(rdds_out* s, T v) { rdds_out_##NAME(s, v); }                    \
    static void read(rdds_in* s, T& v) { v = rdds_in_##NAME(s); }                     \
    static void write_n(rdds_out* s, const T* p, size_t n) { rdds_out_##NAME##_array(s, p, n); } \
    static void read_n(rdds_in* s, T* p, size_t n) { rdds_in_##NAME##_array(s, p, n); }          \
  };

RUSTORB_DDS_PRIM(bool, boolean)
RUSTORB_DDS_PRIM(uint8_t, octet)
RUSTORB_DDS_PRIM(int8_t, int8)
RUSTORB_DDS_PRIM(int16_t, int16)
RUSTORB_DDS_PRIM(uint16_t, uint16)
RUSTORB_DDS_PRIM(int32_t, int32)
RUSTORB_DDS_PRIM(uint32_t, uint32)
RUSTORB_DDS_PRIM(int64_t, int64)
RUSTORB_DDS_PRIM(uint64_t, uint64)
RUSTORB_DDS_PRIM(float, float32)
RUSTORB_DDS_PRIM(double, float64)
RUSTORB_DDS_PRIM(char, char8)
#undef RUSTORB_DDS_PRIM

template <>
struct prim<wchar_t> {
  using value_type = wchar_t;
  static constexpr bool bulk = true;
  static void write(rdds_out* s, wchar_t v) { rdds_out_char16(s, static_cast<uint16_t>(v)); }
  static void read(rdds_in* s, wchar_t& v) { v = static_cast<wchar_t>(rdds_in_char16(s)); }
  static void write_n(rdds_out* s, const wchar_t* p, size_t n) {
    std::vector<uint16_t> u(n);
    for (size_t i = 0; i < n; i++) u[i] = static_cast<uint16_t>(p[i]);
    rdds_out_char16_array(s, u.data(), n);
  }
  static void read_n(rdds_in* s, wchar_t* p, size_t n) {
    std::vector<uint16_t> u(n);
    rdds_in_char16_array(s, u.data(), n);
    for (size_t i = 0; i < n; i++) p[i] = static_cast<wchar_t>(u[i]);
  }
};

template <>
struct prim<long double> {
  using value_type = long double;
  static constexpr bool bulk = false;
  static void write(rdds_out* s, long double v) {
    uint64_t high = 0, low = 0;
    detail::to_binary128(v, high, low);
    rdds_out_float128(s, high, low);
  }
  static void read(rdds_in* s, long double& v) {
    uint64_t high = 0, low = 0;
    rdds_in_float128(s, &high, &low);
    v = detail::from_binary128(high, low);
  }
};

// A struct, union, enum or bitmask: its codec.
template <typename T>
struct type {
  static constexpr bool bulk = false;
  static void write(rdds_out* s, const T& v) { codec<T>::write(s, v); }
  static void read(rdds_in* s, T& v) { codec<T>::read(s, v); }
};

// A string (std::string, or a class derived from it).
struct string {
  static constexpr bool bulk = false;
  static void write(rdds_out* s, const std::string& v) { rdds_out_string(s, v.data(), v.size()); }
  template <typename V>
  static void read(rdds_in* s, V& v) {
    const char* p = nullptr;
    size_t n = 0;
    rdds_in_string(s, &p, &n);
    v.assign(p ? p : "", n);
  }
};

// A wide string (std::wstring, or a class derived from it).
struct wstring {
  static constexpr bool bulk = false;
  static void write(rdds_out* s, const std::wstring& v) {
    std::vector<uint16_t> u = detail::to_utf16(v.data(), v.size());
    rdds_out_wstring(s, u.data(), u.size());
  }
  template <typename V>
  static void read(rdds_in* s, V& v) {
    const uint16_t* p = nullptr;
    size_t n = 0;
    rdds_in_wstring(s, &p, &n);
    std::wstring w = detail::from_utf16(p, n);
    v.assign(w.data(), w.size());
  }
};

// A sequence of E (std::vector, or a class derived from it).
template <typename E>
struct sequence {
  static constexpr bool bulk = false;
  template <typename V>
  static void write(rdds_out* s, const V& v) {
    rdds_out_length(s, static_cast<uint32_t>(v.size()));
    if constexpr (E::bulk) {
      using T = typename E::value_type;
      if constexpr (std::is_same<T, bool>::value) {
        std::unique_ptr<bool[]> b(new bool[v.size() ? v.size() : 1]);
        for (size_t i = 0; i < v.size(); i++) b[i] = v[i];
        E::write_n(s, b.get(), v.size());
      } else {
        E::write_n(s, v.data(), v.size());
      }
    } else {
      for (const auto& e : v) E::write(s, e);
    }
  }
  template <typename V>
  static void read(rdds_in* s, V& v) {
    uint32_t n = rdds_in_length(s);
    if (rdds_in_status(s) != RDDS_RETCODE_OK) n = 0;
    v.clear();
    v.resize(n);
    if constexpr (E::bulk) {
      using T = typename E::value_type;
      if constexpr (std::is_same<T, bool>::value) {
        std::unique_ptr<bool[]> b(new bool[n ? n : 1]);
        E::read_n(s, b.get(), n);
        for (size_t i = 0; i < n; i++) v[i] = b[i];
      } else {
        E::read_n(s, v.data(), n);
      }
    } else {
      for (auto& e : v) E::read(s, e);
    }
  }
};

namespace impl {

// The elements of D nested std::arrays, row by row.
template <size_t D>
struct levels {
  template <typename A, typename F>
  static void each(A& a, F&& f) {
    for (auto& e : a) levels<D - 1>::each(e, f);
  }
  template <typename A>
  static constexpr size_t count() {
    return std::tuple_size<A>::value * levels<D - 1>::template count<typename A::value_type>();
  }
};
template <>
struct levels<0> {
  template <typename A, typename F>
  static void each(A& a, F&& f) {
    f(a);
  }
  template <typename A>
  static constexpr size_t count() {
    return 1;
  }
};

}  // namespace impl

// An array of E with D dimensions: D nested std::arrays.
template <typename E, size_t D>
struct array {
  static constexpr bool bulk = false;
  template <typename V>
  static void write(rdds_out* s, const V& v) {
    if constexpr (E::bulk) {
      using T = typename E::value_type;
      constexpr size_t n = impl::levels<D>::template count<V>();
      std::unique_ptr<T[]> flat(new T[n]());
      size_t i = 0;
      impl::levels<D>::each(v, [&](const T& e) { flat[i++] = e; });
      E::write_n(s, flat.get(), n);
    } else {
      impl::levels<D>::each(v, [&](const auto& e) { E::write(s, e); });
    }
  }
  template <typename V>
  static void read(rdds_in* s, V& v) {
    if constexpr (E::bulk) {
      using T = typename E::value_type;
      constexpr size_t n = impl::levels<D>::template count<V>();
      std::unique_ptr<T[]> flat(new T[n]());
      E::read_n(s, flat.get(), n);
      size_t i = 0;
      impl::levels<D>::each(v, [&](T& e) { e = flat[i++]; });
    } else {
      impl::levels<D>::each(v, [&](auto& e) { E::read(s, e); });
    }
  }
};

// An @optional member of shape E (std::optional).
template <typename E>
struct optional {
  static constexpr bool bulk = false;
  template <typename V>
  static void write(rdds_out* s, const std::optional<V>& v) {
    rdds_out_present(s, v.has_value());
    if (v) E::write(s, *v);
  }
  template <typename V>
  static void read(rdds_in* s, std::optional<V>& v) {
    if (rdds_in_present(s)) {
      V x{};
      E::read(s, x);
      v = std::move(x);
    } else {
      v.reset();
    }
  }
};

// A map of K to V (std::map, or a class derived from it).
template <typename K, typename V>
struct map {
  static constexpr bool bulk = false;
  template <typename M>
  static void write(rdds_out* s, const M& m) {
    rdds_out_length(s, static_cast<uint32_t>(m.size()));
    for (const auto& kv : m) {
      K::write(s, kv.first);
      V::write(s, kv.second);
    }
  }
  template <typename M>
  static void read(rdds_in* s, M& m) {
    uint32_t n = rdds_in_length(s);
    if (rdds_in_status(s) != RDDS_RETCODE_OK) n = 0;
    m.clear();
    for (uint32_t i = 0; i < n; i++) {
      typename M::key_type k{};
      typename M::mapped_type v{};
      K::read(s, k);
      V::read(s, v);
      m.emplace(std::move(k), std::move(v));
    }
  }
};

}  // namespace shape

// ---------------------------------------------------------------------------
// Streams

// An rdds_out, released at the end.
class out_stream {
 public:
  out_stream() { detail::check(rdds_out_new(&s_)); }
  ~out_stream() { rdds_out_release(s_); }
  out_stream(const out_stream&) = delete;
  out_stream& operator=(const out_stream&) = delete;
  rdds_out* get() const { return s_; }

 private:
  rdds_out* s_ = nullptr;
};

// An rdds_in, released at the end.
class in_stream {
 public:
  explicit in_stream(rdds_in* s) : s_(s) {}
  ~in_stream() { rdds_in_release(s_); }
  in_stream(const in_stream&) = delete;
  in_stream& operator=(const in_stream&) = delete;
  rdds_in* get() const { return s_; }

 private:
  rdds_in* s_ = nullptr;
};

// The values of v, in a new stream.
template <typename T>
void write_sample(out_stream& s, const T& v) {
  rdds_out_clear(s.get());
  codec<T>::write(s.get(), v);
}

// v from the values of s; throws if they do not fit.
template <typename T>
void read_sample(rdds_in* s, T& v) {
  codec<T>::read(s, v);
  rdds_status st = rdds_in_status(s);
  if (st != RDDS_RETCODE_OK) detail::raise(st);
}

}  // namespace dds
}  // namespace rustorb

#endif  // RUSTORB_DDS_TYPES_HPP

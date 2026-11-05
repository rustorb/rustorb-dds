// rustorb/dds/core.hpp: the values of dds::core (exceptions, Duration,
// Time, InstanceHandle) of the ISO/IEC C++ PSM for DDS, on the C ABI of
// rustorb-dds (rustorb/dds.h). Included by <dds/dds.hpp>.
//
// SPDX-License-Identifier: MIT OR Apache-2.0
#ifndef RUSTORB_DDS_CORE_HPP
#define RUSTORB_DDS_CORE_HPP

#include <rustorb/dds.h>

#include <chrono>
#include <cstdint>
#include <cstring>
#include <ostream>
#include <stdexcept>
#include <string>
#include <vector>

namespace dds {
namespace core {

// ---------------------------------------------------------------------------
// Exceptions (DDS-PSM-Cxx 1.0, 7.5.4): one class per ReturnCode_t that is
// an error.

class Exception {
 protected:
  Exception() = default;

 public:
  virtual ~Exception() = default;
  virtual const char* what() const noexcept = 0;
};

#define RUSTORB_DDS_EXCEPTION(NAME, BASE)                                  \
  class NAME : public Exception, public BASE {                             \
   public:                                                                 \
    explicit NAME(const std::string& msg) : BASE(msg) {}                   \
    const char* what() const noexcept override { return BASE::what(); }   \
  };

namespace detail {
// std::exception has no constructor taking a message.
class message_error : public std::exception {
 public:
  explicit message_error(const std::string& msg) : msg_(msg) {}
  const char* what() const noexcept override { return msg_.c_str(); }

 private:
  std::string msg_;
};
}  // namespace detail

RUSTORB_DDS_EXCEPTION(Error, detail::message_error)
RUSTORB_DDS_EXCEPTION(TimeoutError, detail::message_error)
RUSTORB_DDS_EXCEPTION(InvalidArgumentError, std::invalid_argument)
RUSTORB_DDS_EXCEPTION(UnsupportedError, std::logic_error)
RUSTORB_DDS_EXCEPTION(AlreadyClosedError, std::logic_error)
RUSTORB_DDS_EXCEPTION(IllegalOperationError, std::logic_error)
RUSTORB_DDS_EXCEPTION(NotEnabledError, std::logic_error)
RUSTORB_DDS_EXCEPTION(PreconditionNotMetError, std::logic_error)
RUSTORB_DDS_EXCEPTION(ImmutablePolicyError, std::logic_error)
RUSTORB_DDS_EXCEPTION(InconsistentPolicyError, std::logic_error)
RUSTORB_DDS_EXCEPTION(InvalidDataError, std::logic_error)
RUSTORB_DDS_EXCEPTION(OutOfResourcesError, std::runtime_error)
RUSTORB_DDS_EXCEPTION(InvalidDowncastError, std::runtime_error)
RUSTORB_DDS_EXCEPTION(NullReferenceError, std::runtime_error)

#undef RUSTORB_DDS_EXCEPTION

// dds::core::null: what an entity that refers to nothing compares equal to.
struct null_type {};
inline constexpr null_type null{};

// ---------------------------------------------------------------------------
// Duration and Time (DDS-PSM-Cxx 1.0, 7.5.3)

class Duration {
 public:
  Duration() = default;
  explicit Duration(int32_t sec, uint32_t nanosec = 0) : sec_(sec), nanosec_(nanosec) {}

  static Duration zero() { return Duration(); }
  static Duration infinite() {
    return Duration(RDDS_DURATION_INFINITE_SEC, RDDS_DURATION_INFINITE_NSEC);
  }
  static Duration from_microsecs(uint64_t us) {
    return Duration(static_cast<int32_t>(us / 1000000), static_cast<uint32_t>(us % 1000000) * 1000);
  }
  static Duration from_millisecs(uint64_t ms) {
    return Duration(static_cast<int32_t>(ms / 1000), static_cast<uint32_t>(ms % 1000) * 1000000);
  }
  static Duration from_secs(double s) {
    int32_t whole = static_cast<int32_t>(s);
    return Duration(whole, static_cast<uint32_t>((s - whole) * 1e9));
  }

  int32_t sec() const { return sec_; }
  void sec(int32_t s) { sec_ = s; }
  uint32_t nanosec() const { return nanosec_; }
  void nanosec(uint32_t ns) { nanosec_ = ns; }

  bool is_infinite() const {
    return sec_ == RDDS_DURATION_INFINITE_SEC && nanosec_ == RDDS_DURATION_INFINITE_NSEC;
  }
  int64_t to_millisecs() const {
    return static_cast<int64_t>(sec_) * 1000 + nanosec_ / 1000000;
  }
  int64_t to_microsecs() const {
    return static_cast<int64_t>(sec_) * 1000000 + nanosec_ / 1000;
  }
  double to_secs() const { return sec_ + nanosec_ / 1e9; }

  int compare(const Duration& o) const {
    if (sec_ != o.sec_) return sec_ < o.sec_ ? -1 : 1;
    if (nanosec_ != o.nanosec_) return nanosec_ < o.nanosec_ ? -1 : 1;
    return 0;
  }
  friend bool operator==(const Duration& a, const Duration& b) { return a.compare(b) == 0; }
  friend bool operator!=(const Duration& a, const Duration& b) { return a.compare(b) != 0; }
  friend bool operator<(const Duration& a, const Duration& b) { return a.compare(b) < 0; }
  friend bool operator<=(const Duration& a, const Duration& b) { return a.compare(b) <= 0; }
  friend bool operator>(const Duration& a, const Duration& b) { return a.compare(b) > 0; }
  friend bool operator>=(const Duration& a, const Duration& b) { return a.compare(b) >= 0; }

  friend Duration operator+(const Duration& a, const Duration& b) {
    int64_t ns = static_cast<int64_t>(a.nanosec_) + b.nanosec_;
    return Duration(a.sec_ + b.sec_ + static_cast<int32_t>(ns / 1000000000),
                    static_cast<uint32_t>(ns % 1000000000));
  }
  friend Duration operator-(const Duration& a, const Duration& b) {
    int64_t ns = (static_cast<int64_t>(a.sec_) - b.sec_) * 1000000000 +
                 (static_cast<int64_t>(a.nanosec_) - b.nanosec_);
    return Duration(static_cast<int32_t>(ns / 1000000000),
                    static_cast<uint32_t>(ns % 1000000000));
  }

  rdds_duration c() const { return rdds_duration{sec_, nanosec_}; }

 private:
  int32_t sec_ = 0;
  uint32_t nanosec_ = 0;
};

class Time {
 public:
  Time() = default;
  explicit Time(int32_t sec, uint32_t nanosec = 0) : sec_(sec), nanosec_(nanosec) {}

  static Time invalid() { return Time(RDDS_TIME_INVALID_SEC, RDDS_TIME_INVALID_NSEC); }
  static Time from_millisecs(int64_t ms) {
    return Time(static_cast<int32_t>(ms / 1000), static_cast<uint32_t>(ms % 1000) * 1000000);
  }
  static Time from_microsecs(int64_t us) {
    return Time(static_cast<int32_t>(us / 1000000), static_cast<uint32_t>(us % 1000000) * 1000);
  }
  static Time from_secs(double s) {
    int32_t whole = static_cast<int32_t>(s);
    return Time(whole, static_cast<uint32_t>((s - whole) * 1e9));
  }
  // rustorb: the time of the system clock.
  static Time now() {
    auto ns = std::chrono::duration_cast<std::chrono::nanoseconds>(
                  std::chrono::system_clock::now().time_since_epoch())
                  .count();
    return Time(static_cast<int32_t>(ns / 1000000000), static_cast<uint32_t>(ns % 1000000000));
  }

  int32_t sec() const { return sec_; }
  void sec(int32_t s) { sec_ = s; }
  uint32_t nanosec() const { return nanosec_; }
  void nanosec(uint32_t ns) { nanosec_ = ns; }
  bool is_invalid() const {
    return sec_ == RDDS_TIME_INVALID_SEC && nanosec_ == RDDS_TIME_INVALID_NSEC;
  }
  int64_t to_millisecs() const { return static_cast<int64_t>(sec_) * 1000 + nanosec_ / 1000000; }
  int64_t to_microsecs() const { return static_cast<int64_t>(sec_) * 1000000 + nanosec_ / 1000; }
  double to_secs() const { return sec_ + nanosec_ / 1e9; }

  int compare(const Time& o) const {
    if (sec_ != o.sec_) return sec_ < o.sec_ ? -1 : 1;
    if (nanosec_ != o.nanosec_) return nanosec_ < o.nanosec_ ? -1 : 1;
    return 0;
  }
  friend bool operator==(const Time& a, const Time& b) { return a.compare(b) == 0; }
  friend bool operator!=(const Time& a, const Time& b) { return a.compare(b) != 0; }
  friend bool operator<(const Time& a, const Time& b) { return a.compare(b) < 0; }
  friend bool operator<=(const Time& a, const Time& b) { return a.compare(b) <= 0; }
  friend bool operator>(const Time& a, const Time& b) { return a.compare(b) > 0; }
  friend bool operator>=(const Time& a, const Time& b) { return a.compare(b) >= 0; }

  rdds_time c() const { return rdds_time{sec_, nanosec_}; }
  static Time from_c(const rdds_time& t) { return Time(t.sec, t.nanosec); }

 private:
  int32_t sec_ = 0;
  uint32_t nanosec_ = 0;
};

// ---------------------------------------------------------------------------
// InstanceHandle (DDS-PSM-Cxx 1.0, 7.5.6): an instance, or a matched
// entity (its GUID).

class InstanceHandle {
 public:
  InstanceHandle() { std::memset(v_, 0, sizeof v_); }
  InstanceHandle(const null_type&) : InstanceHandle() {}
  explicit InstanceHandle(const rdds_instance_handle& h) { std::memcpy(v_, h.value, sizeof v_); }
  explicit InstanceHandle(const rdds_guid& g) { std::memcpy(v_, g.value, sizeof v_); }

  static InstanceHandle nil() { return InstanceHandle(); }
  bool is_nil() const {
    for (uint8_t b : v_)
      if (b) return false;
    return true;
  }
  InstanceHandle& operator=(const null_type&) { return *this = InstanceHandle(); }

  friend bool operator==(const InstanceHandle& a, const InstanceHandle& b) {
    return std::memcmp(a.v_, b.v_, sizeof a.v_) == 0;
  }
  friend bool operator!=(const InstanceHandle& a, const InstanceHandle& b) { return !(a == b); }
  friend bool operator<(const InstanceHandle& a, const InstanceHandle& b) {
    return std::memcmp(a.v_, b.v_, sizeof a.v_) < 0;
  }
  friend bool operator==(const InstanceHandle& a, const null_type&) { return a.is_nil(); }
  friend bool operator!=(const InstanceHandle& a, const null_type&) { return !a.is_nil(); }

  rdds_instance_handle c() const {
    rdds_instance_handle h;
    std::memcpy(h.value, v_, sizeof v_);
    return h;
  }
  // rustorb: the 16 octets (a GUID for a matched entity).
  const uint8_t* value() const { return v_; }

  friend std::ostream& operator<<(std::ostream& os, const InstanceHandle& h) {
    static const char hex[] = "0123456789abcdef";
    for (uint8_t b : h.v_) os << hex[b >> 4] << hex[b & 15];
    return os;
  }

 private:
  uint8_t v_[16];
};

using InstanceHandleSeq = std::vector<InstanceHandle>;
using ByteSeq = std::vector<uint8_t>;
using StringSeq = std::vector<std::string>;

// LENGTH_UNLIMITED (DDS-PSM-Cxx 1.0, 7.5.5).
inline constexpr int32_t LENGTH_UNLIMITED = RDDS_LENGTH_UNLIMITED;

}  // namespace core
}  // namespace dds

namespace rustorb {
namespace dds {
namespace detail {

// Throws the exception of a failed rdds_status, with the library's message.
[[noreturn]] inline void raise(rdds_status s, const char* what = nullptr) {
  std::string msg = what ? std::string(what) : std::string(rdds_last_error_message());
  if (msg.empty()) msg = "rustorb-dds error " + std::to_string(s);
  namespace c = ::dds::core;
  switch (s) {
    case RDDS_RETCODE_UNSUPPORTED: throw c::UnsupportedError(msg);
    case RDDS_RETCODE_BAD_PARAMETER: throw c::InvalidArgumentError(msg);
    case RDDS_RETCODE_PRECONDITION_NOT_MET: throw c::PreconditionNotMetError(msg);
    case RDDS_RETCODE_OUT_OF_RESOURCES: throw c::OutOfResourcesError(msg);
    case RDDS_RETCODE_NOT_ENABLED: throw c::NotEnabledError(msg);
    case RDDS_RETCODE_IMMUTABLE_POLICY: throw c::ImmutablePolicyError(msg);
    case RDDS_RETCODE_INCONSISTENT_POLICY: throw c::InconsistentPolicyError(msg);
    case RDDS_RETCODE_ALREADY_DELETED: throw c::AlreadyClosedError(msg);
    case RDDS_RETCODE_TIMEOUT: throw c::TimeoutError(msg);
    case RDDS_RETCODE_ILLEGAL_OPERATION: throw c::IllegalOperationError(msg);
    default: throw c::Error(msg);
  }
}

inline void check(rdds_status s) {
  if (s != RDDS_RETCODE_OK) raise(s);
}

}  // namespace detail
}  // namespace dds
}  // namespace rustorb

#endif  // RUSTORB_DDS_CORE_HPP

// rustorb/dds/policy.hpp: the QoS policies, the QoS of each entity and
// the statuses of the ISO/IEC C++ PSM for DDS (dds::core::policy,
// dds::*::qos, dds::core::status, dds::sub::status), on the C ABI of
// rustorb-dds. Included by <dds/dds.hpp>.
//
// SPDX-License-Identifier: MIT OR Apache-2.0
#ifndef RUSTORB_DDS_POLICY_HPP
#define RUSTORB_DDS_POLICY_HPP

#include <rustorb/dds/core.hpp>

#include <bitset>
#include <cstdint>
#include <string>
#include <tuple>
#include <utility>
#include <vector>

namespace dds {
namespace core {
namespace policy {

// ---------------------------------------------------------------------------
// Policies (DDS-PSM-Cxx 1.0, 7.5.7; DDS 1.4, 2.2.3)

struct ReliabilityKind {
  enum Type { BEST_EFFORT = RDDS_BEST_EFFORT_RELIABILITY_QOS, RELIABLE = RDDS_RELIABLE_RELIABILITY_QOS };
};

class Reliability {
 public:
  explicit Reliability(ReliabilityKind::Type kind = ReliabilityKind::BEST_EFFORT,
                       const Duration& max_blocking_time = Duration::from_millisecs(100))
      : kind_(kind), max_blocking_time_(max_blocking_time) {}
  static Reliability Reliable(const Duration& max_blocking_time = Duration::from_millisecs(100)) {
    return Reliability(ReliabilityKind::RELIABLE, max_blocking_time);
  }
  static Reliability BestEffort(const Duration& max_blocking_time = Duration::from_millisecs(100)) {
    return Reliability(ReliabilityKind::BEST_EFFORT, max_blocking_time);
  }
  ReliabilityKind::Type kind() const { return kind_; }
  Reliability& kind(ReliabilityKind::Type k) { kind_ = k; return *this; }
  const Duration& max_blocking_time() const { return max_blocking_time_; }
  Reliability& max_blocking_time(const Duration& d) { max_blocking_time_ = d; return *this; }
  friend bool operator==(const Reliability& a, const Reliability& b) {
    return a.kind_ == b.kind_ && a.max_blocking_time_ == b.max_blocking_time_;
  }

 private:
  ReliabilityKind::Type kind_;
  Duration max_blocking_time_;
};

struct HistoryKind {
  enum Type { KEEP_LAST = RDDS_KEEP_LAST_HISTORY_QOS, KEEP_ALL = RDDS_KEEP_ALL_HISTORY_QOS };
};

class History {
 public:
  explicit History(HistoryKind::Type kind = HistoryKind::KEEP_LAST, int32_t depth = 1)
      : kind_(kind), depth_(depth) {}
  static History KeepAll() { return History(HistoryKind::KEEP_ALL, 1); }
  static History KeepLast(uint32_t depth) { return History(HistoryKind::KEEP_LAST, static_cast<int32_t>(depth)); }
  HistoryKind::Type kind() const { return kind_; }
  History& kind(HistoryKind::Type k) { kind_ = k; return *this; }
  int32_t depth() const { return depth_; }
  History& depth(int32_t d) { depth_ = d; return *this; }
  friend bool operator==(const History& a, const History& b) {
    return a.kind_ == b.kind_ && a.depth_ == b.depth_;
  }

 private:
  HistoryKind::Type kind_;
  int32_t depth_;
};

struct DurabilityKind {
  enum Type {
    VOLATILE = RDDS_VOLATILE_DURABILITY_QOS,
    TRANSIENT_LOCAL = RDDS_TRANSIENT_LOCAL_DURABILITY_QOS,
    TRANSIENT = RDDS_TRANSIENT_DURABILITY_QOS,
    PERSISTENT = RDDS_PERSISTENT_DURABILITY_QOS
  };
};

class Durability {
 public:
  explicit Durability(DurabilityKind::Type kind = DurabilityKind::VOLATILE) : kind_(kind) {}
  static Durability Volatile() { return Durability(DurabilityKind::VOLATILE); }
  static Durability TransientLocal() { return Durability(DurabilityKind::TRANSIENT_LOCAL); }
  static Durability Transient() { return Durability(DurabilityKind::TRANSIENT); }
  static Durability Persistent() { return Durability(DurabilityKind::PERSISTENT); }
  DurabilityKind::Type kind() const { return kind_; }
  Durability& kind(DurabilityKind::Type k) { kind_ = k; return *this; }
  friend bool operator==(const Durability& a, const Durability& b) { return a.kind_ == b.kind_; }

 private:
  DurabilityKind::Type kind_;
};

class Deadline {
 public:
  explicit Deadline(const Duration& period = Duration::infinite()) : period_(period) {}
  const Duration& period() const { return period_; }
  Deadline& period(const Duration& d) { period_ = d; return *this; }
  friend bool operator==(const Deadline& a, const Deadline& b) { return a.period_ == b.period_; }

 private:
  Duration period_;
};

struct LivelinessKind {
  enum Type {
    AUTOMATIC = RDDS_AUTOMATIC_LIVELINESS_QOS,
    MANUAL_BY_PARTICIPANT = RDDS_MANUAL_BY_PARTICIPANT_LIVELINESS_QOS,
    MANUAL_BY_TOPIC = RDDS_MANUAL_BY_TOPIC_LIVELINESS_QOS
  };
};

class Liveliness {
 public:
  explicit Liveliness(LivelinessKind::Type kind = LivelinessKind::AUTOMATIC,
                      const Duration& lease_duration = Duration::infinite())
      : kind_(kind), lease_duration_(lease_duration) {}
  static Liveliness Automatic() { return Liveliness(LivelinessKind::AUTOMATIC); }
  static Liveliness ManualByParticipant(const Duration& lease = Duration::infinite()) {
    return Liveliness(LivelinessKind::MANUAL_BY_PARTICIPANT, lease);
  }
  static Liveliness ManualByTopic(const Duration& lease = Duration::infinite()) {
    return Liveliness(LivelinessKind::MANUAL_BY_TOPIC, lease);
  }
  LivelinessKind::Type kind() const { return kind_; }
  Liveliness& kind(LivelinessKind::Type k) { kind_ = k; return *this; }
  const Duration& lease_duration() const { return lease_duration_; }
  Liveliness& lease_duration(const Duration& d) { lease_duration_ = d; return *this; }
  friend bool operator==(const Liveliness& a, const Liveliness& b) {
    return a.kind_ == b.kind_ && a.lease_duration_ == b.lease_duration_;
  }

 private:
  LivelinessKind::Type kind_;
  Duration lease_duration_;
};

class ResourceLimits {
 public:
  explicit ResourceLimits(int32_t max_samples = LENGTH_UNLIMITED,
                          int32_t max_instances = LENGTH_UNLIMITED,
                          int32_t max_samples_per_instance = LENGTH_UNLIMITED)
      : max_samples_(max_samples),
        max_instances_(max_instances),
        max_samples_per_instance_(max_samples_per_instance) {}
  int32_t max_samples() const { return max_samples_; }
  ResourceLimits& max_samples(int32_t v) { max_samples_ = v; return *this; }
  int32_t max_instances() const { return max_instances_; }
  ResourceLimits& max_instances(int32_t v) { max_instances_ = v; return *this; }
  int32_t max_samples_per_instance() const { return max_samples_per_instance_; }
  ResourceLimits& max_samples_per_instance(int32_t v) { max_samples_per_instance_ = v; return *this; }
  friend bool operator==(const ResourceLimits& a, const ResourceLimits& b) {
    return a.max_samples_ == b.max_samples_ && a.max_instances_ == b.max_instances_ &&
           a.max_samples_per_instance_ == b.max_samples_per_instance_;
  }

 private:
  int32_t max_samples_, max_instances_, max_samples_per_instance_;
};

class Partition {
 public:
  Partition() = default;
  explicit Partition(const std::string& name) : name_{name} {}
  explicit Partition(const StringSeq& names) : name_(names) {}
  const StringSeq& name() const { return name_; }
  Partition& name(const std::string& n) { name_ = {n}; return *this; }
  Partition& name(const StringSeq& n) { name_ = n; return *this; }
  friend bool operator==(const Partition& a, const Partition& b) { return a.name_ == b.name_; }

 private:
  StringSeq name_;
};

class UserData {
 public:
  UserData() = default;
  explicit UserData(const ByteSeq& value) : value_(value) {}
  const ByteSeq& value() const { return value_; }
  UserData& value(const ByteSeq& v) { value_ = v; return *this; }
  friend bool operator==(const UserData& a, const UserData& b) { return a.value_ == b.value_; }

 private:
  ByteSeq value_;
};

class WriterDataLifecycle {
 public:
  explicit WriterDataLifecycle(bool autodispose = true) : autodispose_(autodispose) {}
  static WriterDataLifecycle AutoDisposeUnregisteredInstances() { return WriterDataLifecycle(true); }
  static WriterDataLifecycle ManuallyDisposeUnregisteredInstances() { return WriterDataLifecycle(false); }
  bool autodispose_unregistered_instances() const { return autodispose_; }
  WriterDataLifecycle& autodispose_unregistered_instances(bool b) { autodispose_ = b; return *this; }
  bool autodispose() const { return autodispose_; }
  friend bool operator==(const WriterDataLifecycle& a, const WriterDataLifecycle& b) {
    return a.autodispose_ == b.autodispose_;
  }

 private:
  bool autodispose_;
};

// DATA_REPRESENTATION (DDS-XTypes 1.3, 7.6.3.1).
using DataRepresentationId = int16_t;
inline constexpr DataRepresentationId XCDR_DATA_REPRESENTATION = RDDS_XCDR_DATA_REPRESENTATION;
inline constexpr DataRepresentationId XML_DATA_REPRESENTATION = 1;
inline constexpr DataRepresentationId XCDR2_DATA_REPRESENTATION = RDDS_XCDR2_DATA_REPRESENTATION;
using DataRepresentationIdSeq = std::vector<DataRepresentationId>;

class DataRepresentation {
 public:
  DataRepresentation() = default;
  explicit DataRepresentation(const DataRepresentationIdSeq& value) : value_(value) {}
  const DataRepresentationIdSeq& value() const { return value_; }
  DataRepresentation& value(const DataRepresentationIdSeq& v) { value_ = v; return *this; }
  friend bool operator==(const DataRepresentation& a, const DataRepresentation& b) {
    return a.value_ == b.value_;
  }

 private:
  DataRepresentationIdSeq value_;
};

// TYPE_CONSISTENCY_ENFORCEMENT (DDS-XTypes 1.3, 7.6.3.4).
struct TypeConsistencyKind {
  enum Type {
    DISALLOW_TYPE_COERCION = RDDS_DISALLOW_TYPE_COERCION,
    ALLOW_TYPE_COERCION = RDDS_ALLOW_TYPE_COERCION
  };
};

class TypeConsistencyEnforcement {
 public:
  explicit TypeConsistencyEnforcement(
      TypeConsistencyKind::Type kind = TypeConsistencyKind::ALLOW_TYPE_COERCION)
      : kind_(kind) {}
  TypeConsistencyKind::Type kind() const { return kind_; }
  TypeConsistencyEnforcement& kind(TypeConsistencyKind::Type k) { kind_ = k; return *this; }
  bool ignore_sequence_bounds() const { return ignore_sequence_bounds_; }
  TypeConsistencyEnforcement& ignore_sequence_bounds(bool b) { ignore_sequence_bounds_ = b; return *this; }
  bool ignore_string_bounds() const { return ignore_string_bounds_; }
  TypeConsistencyEnforcement& ignore_string_bounds(bool b) { ignore_string_bounds_ = b; return *this; }
  bool ignore_member_names() const { return ignore_member_names_; }
  TypeConsistencyEnforcement& ignore_member_names(bool b) { ignore_member_names_ = b; return *this; }
  bool prevent_type_widening() const { return prevent_type_widening_; }
  TypeConsistencyEnforcement& prevent_type_widening(bool b) { prevent_type_widening_ = b; return *this; }
  bool force_type_validation() const { return force_type_validation_; }
  TypeConsistencyEnforcement& force_type_validation(bool b) { force_type_validation_ = b; return *this; }
  friend bool operator==(const TypeConsistencyEnforcement& a, const TypeConsistencyEnforcement& b) {
    return a.kind_ == b.kind_ && a.ignore_sequence_bounds_ == b.ignore_sequence_bounds_ &&
           a.ignore_string_bounds_ == b.ignore_string_bounds_ &&
           a.ignore_member_names_ == b.ignore_member_names_ &&
           a.prevent_type_widening_ == b.prevent_type_widening_ &&
           a.force_type_validation_ == b.force_type_validation_;
  }

 private:
  TypeConsistencyKind::Type kind_;
  bool ignore_sequence_bounds_ = true;
  bool ignore_string_bounds_ = true;
  bool ignore_member_names_ = false;
  bool prevent_type_widening_ = false;
  bool force_type_validation_ = false;
};

// QosPolicyId (DDS 1.4, 2.2.3) and the counts of incompatible QoS statuses.
using QosPolicyId = int32_t;

class QosPolicyCount {
 public:
  QosPolicyCount(QosPolicyId policy_id, int32_t count) : policy_id_(policy_id), count_(count) {}
  QosPolicyId policy_id() const { return policy_id_; }
  int32_t count() const { return count_; }

 private:
  QosPolicyId policy_id_;
  int32_t count_;
};
using QosPolicyCountSeq = std::vector<QosPolicyCount>;

template <typename P>
struct policy_id;
#define RUSTORB_DDS_POLICY_ID(P, ID) \
  template <>                        \
  struct policy_id<P> {              \
    static const QosPolicyId value = ID; \
  };
RUSTORB_DDS_POLICY_ID(UserData, 1)
RUSTORB_DDS_POLICY_ID(Durability, 2)
RUSTORB_DDS_POLICY_ID(Deadline, 4)
RUSTORB_DDS_POLICY_ID(Liveliness, 8)
RUSTORB_DDS_POLICY_ID(Reliability, 11)
RUSTORB_DDS_POLICY_ID(Partition, 10)
RUSTORB_DDS_POLICY_ID(History, 13)
RUSTORB_DDS_POLICY_ID(ResourceLimits, 14)
RUSTORB_DDS_POLICY_ID(WriterDataLifecycle, 16)
RUSTORB_DDS_POLICY_ID(DataRepresentation, 23)
RUSTORB_DDS_POLICY_ID(TypeConsistencyEnforcement, 24)
#undef RUSTORB_DDS_POLICY_ID

}  // namespace policy

// ---------------------------------------------------------------------------
// The QoS of an entity: a set of policies (DDS-PSM-Cxx 1.0, 7.5.8).

template <typename Self, typename... P>
class EntityQos {
 public:
  template <typename X>
  const X& policy() const { return std::get<X>(p_); }
  template <typename X>
  X& policy() { return std::get<X>(p_); }
  template <typename X>
  Self& policy(const X& x) {
    std::get<X>(p_) = x;
    return static_cast<Self&>(*this);
  }
  template <typename X>
  Self& operator<<(const X& x) { return policy(x); }
  template <typename X>
  const Self& operator>>(X& x) const {
    x = policy<X>();
    return static_cast<const Self&>(*this);
  }
  friend bool operator==(const EntityQos& a, const EntityQos& b) { return a.p_ == b.p_; }
  friend bool operator!=(const EntityQos& a, const EntityQos& b) { return !(a.p_ == b.p_); }

 protected:
  std::tuple<P...> p_;
};

}  // namespace core

namespace domain {
namespace qos {
class DomainParticipantQos
    : public ::dds::core::EntityQos<DomainParticipantQos, ::dds::core::policy::UserData> {};
}  // namespace qos
}  // namespace domain

namespace topic {
namespace qos {
class TopicQos
    : public ::dds::core::EntityQos<
          TopicQos, ::dds::core::policy::Reliability, ::dds::core::policy::History,
          ::dds::core::policy::Durability, ::dds::core::policy::Deadline,
          ::dds::core::policy::Liveliness, ::dds::core::policy::ResourceLimits,
          ::dds::core::policy::DataRepresentation> {};
}  // namespace qos
}  // namespace topic

namespace pub {
namespace qos {

class PublisherQos
    : public ::dds::core::EntityQos<PublisherQos, ::dds::core::policy::Partition> {};

class DataWriterQos
    : public ::dds::core::EntityQos<
          DataWriterQos, ::dds::core::policy::Reliability, ::dds::core::policy::History,
          ::dds::core::policy::Durability, ::dds::core::policy::Deadline,
          ::dds::core::policy::Liveliness, ::dds::core::policy::ResourceLimits,
          ::dds::core::policy::UserData, ::dds::core::policy::DataRepresentation,
          ::dds::core::policy::WriterDataLifecycle> {
 public:
  // The defaults of rustorb-dds (RELIABLE, KEEP_LAST 1...).
  DataWriterQos() {
    rdds_writer_qos c;
    rustorb::dds::detail::check(rdds_writer_qos_default(&c));
    namespace p = ::dds::core::policy;
    policy(p::Reliability(static_cast<p::ReliabilityKind::Type>(c.reliability_kind),
                          ::dds::core::Duration(c.max_blocking_time.sec, c.max_blocking_time.nanosec)));
    policy(p::History(static_cast<p::HistoryKind::Type>(c.history_kind), c.history_depth));
    policy(p::ResourceLimits(c.max_samples, c.max_instances, c.max_samples_per_instance));
    policy(p::Durability(static_cast<p::DurabilityKind::Type>(c.durability_kind)));
    policy(p::WriterDataLifecycle(c.autodispose_unregistered_instances));
  }
  // The policies of a topic's QoS, the others the defaults
  // (copy_from_topic_qos, DDS 1.4, 2.2.2.4.1.13).
  DataWriterQos(const ::dds::topic::qos::TopicQos& t) : DataWriterQos() { *this = t; }
  DataWriterQos& operator=(const ::dds::topic::qos::TopicQos& t) {
    namespace p = ::dds::core::policy;
    policy(t.policy<p::Reliability>());
    policy(t.policy<p::History>());
    policy(t.policy<p::Durability>());
    policy(t.policy<p::Deadline>());
    policy(t.policy<p::Liveliness>());
    policy(t.policy<p::ResourceLimits>());
    policy(t.policy<p::DataRepresentation>());
    return *this;
  }
};

}  // namespace qos
}  // namespace pub

namespace sub {
namespace qos {

class SubscriberQos
    : public ::dds::core::EntityQos<SubscriberQos, ::dds::core::policy::Partition> {};

class DataReaderQos
    : public ::dds::core::EntityQos<
          DataReaderQos, ::dds::core::policy::Reliability, ::dds::core::policy::History,
          ::dds::core::policy::Durability, ::dds::core::policy::Deadline,
          ::dds::core::policy::Liveliness, ::dds::core::policy::ResourceLimits,
          ::dds::core::policy::UserData, ::dds::core::policy::DataRepresentation,
          ::dds::core::policy::TypeConsistencyEnforcement> {
 public:
  // The defaults of rustorb-dds (BEST_EFFORT, KEEP_LAST 1, XCDR and XCDR2...).
  DataReaderQos() {
    rdds_reader_qos c;
    rustorb::dds::detail::check(rdds_reader_qos_default(&c));
    namespace p = ::dds::core::policy;
    policy(p::Reliability(static_cast<p::ReliabilityKind::Type>(c.reliability_kind),
                          ::dds::core::Duration(c.max_blocking_time.sec, c.max_blocking_time.nanosec)));
    policy(p::History(static_cast<p::HistoryKind::Type>(c.history_kind), c.history_depth));
    policy(p::ResourceLimits(c.max_samples, c.max_instances, c.max_samples_per_instance));
    policy(p::Durability(static_cast<p::DurabilityKind::Type>(c.durability_kind)));
    policy(p::DataRepresentation(p::DataRepresentationIdSeq(
        c.data_representation, c.data_representation + c.data_representation_len)));
    p::TypeConsistencyEnforcement tce(static_cast<p::TypeConsistencyKind::Type>(c.type_consistency_kind));
    tce.ignore_sequence_bounds(c.ignore_sequence_bounds)
        .ignore_string_bounds(c.ignore_string_bounds)
        .ignore_member_names(c.ignore_member_names)
        .prevent_type_widening(c.prevent_type_widening)
        .force_type_validation(c.force_type_validation);
    policy(tce);
  }
  DataReaderQos(const ::dds::topic::qos::TopicQos& t) : DataReaderQos() { *this = t; }
  DataReaderQos& operator=(const ::dds::topic::qos::TopicQos& t) {
    namespace p = ::dds::core::policy;
    policy(t.policy<p::Reliability>());
    policy(t.policy<p::History>());
    policy(t.policy<p::Durability>());
    policy(t.policy<p::Deadline>());
    policy(t.policy<p::Liveliness>());
    policy(t.policy<p::ResourceLimits>());
    if (!t.policy<p::DataRepresentation>().value().empty()) policy(t.policy<p::DataRepresentation>());
    return *this;
  }
};

}  // namespace qos
}  // namespace sub

// ---------------------------------------------------------------------------
// Statuses (DDS-PSM-Cxx 1.0, 7.5.9; DDS 1.4, 2.2.4.1)

namespace core {
namespace status {

class StatusMask : public std::bitset<32> {
 public:
  StatusMask() = default;
  explicit StatusMask(uint64_t mask) : std::bitset<32>(static_cast<unsigned long>(mask & 0xffffffffu)) {}
  StatusMask(const std::bitset<32>& b) : std::bitset<32>(b) {}
  static StatusMask all() { return StatusMask(0x7fe7u); }
  static StatusMask none() { return StatusMask(0u); }
  static StatusMask inconsistent_topic() { return StatusMask(RDDS_INCONSISTENT_TOPIC_STATUS); }
  static StatusMask offered_deadline_missed() { return StatusMask(RDDS_OFFERED_DEADLINE_MISSED_STATUS); }
  static StatusMask requested_deadline_missed() { return StatusMask(RDDS_REQUESTED_DEADLINE_MISSED_STATUS); }
  static StatusMask offered_incompatible_qos() { return StatusMask(RDDS_OFFERED_INCOMPATIBLE_QOS_STATUS); }
  static StatusMask requested_incompatible_qos() { return StatusMask(RDDS_REQUESTED_INCOMPATIBLE_QOS_STATUS); }
  static StatusMask sample_lost() { return StatusMask(RDDS_SAMPLE_LOST_STATUS); }
  static StatusMask sample_rejected() { return StatusMask(RDDS_SAMPLE_REJECTED_STATUS); }
  static StatusMask data_on_readers() { return StatusMask(RDDS_DATA_ON_READERS_STATUS); }
  static StatusMask data_available() { return StatusMask(RDDS_DATA_AVAILABLE_STATUS); }
  static StatusMask liveliness_lost() { return StatusMask(RDDS_LIVELINESS_LOST_STATUS); }
  static StatusMask liveliness_changed() { return StatusMask(RDDS_LIVELINESS_CHANGED_STATUS); }
  static StatusMask publication_matched() { return StatusMask(RDDS_PUBLICATION_MATCHED_STATUS); }
  static StatusMask subscription_matched() { return StatusMask(RDDS_SUBSCRIPTION_MATCHED_STATUS); }
  StatusMask& operator<<(const StatusMask& m) {
    *this |= m;
    return *this;
  }
  friend StatusMask operator|(const StatusMask& a, const StatusMask& b) {
    return StatusMask(static_cast<const std::bitset<32>&>(a) | static_cast<const std::bitset<32>&>(b));
  }
  friend StatusMask operator&(const StatusMask& a, const StatusMask& b) {
    return StatusMask(static_cast<const std::bitset<32>&>(a) & static_cast<const std::bitset<32>&>(b));
  }
  uint32_t c() const { return static_cast<uint32_t>(to_ulong()); }
};

class SampleRejectedState {
 public:
  enum Kind {
    NOT_REJECTED = RDDS_NOT_REJECTED,
    REJECTED_BY_INSTANCES_LIMIT = RDDS_REJECTED_BY_INSTANCES_LIMIT,
    REJECTED_BY_SAMPLES_LIMIT = RDDS_REJECTED_BY_SAMPLES_LIMIT,
    REJECTED_BY_SAMPLES_PER_INSTANCE_LIMIT = RDDS_REJECTED_BY_SAMPLES_PER_INSTANCE_LIMIT
  };
  explicit SampleRejectedState(int32_t k = NOT_REJECTED) : k_(k) {}
  static SampleRejectedState not_rejected() { return SampleRejectedState(NOT_REJECTED); }
  static SampleRejectedState rejected_by_instances_limit() { return SampleRejectedState(REJECTED_BY_INSTANCES_LIMIT); }
  static SampleRejectedState rejected_by_samples_limit() { return SampleRejectedState(REJECTED_BY_SAMPLES_LIMIT); }
  static SampleRejectedState rejected_by_samples_per_instance_limit() {
    return SampleRejectedState(REJECTED_BY_SAMPLES_PER_INSTANCE_LIMIT);
  }
  int32_t kind() const { return k_; }
  friend bool operator==(const SampleRejectedState& a, const SampleRejectedState& b) { return a.k_ == b.k_; }
  friend bool operator!=(const SampleRejectedState& a, const SampleRejectedState& b) { return a.k_ != b.k_; }

 private:
  int32_t k_;
};

class CountStatus {
 public:
  CountStatus() = default;
  explicit CountStatus(const rdds_count_status& s) : s_(s) {}
  int32_t total_count() const { return s_.total_count; }
  int32_t total_count_change() const { return s_.total_count_change; }

 protected:
  rdds_count_status s_{};
};

class InconsistentTopicStatus : public CountStatus {
  using CountStatus::CountStatus;
};
class LivelinessLostStatus : public CountStatus {
  using CountStatus::CountStatus;
};
class SampleLostStatus : public CountStatus {
  using CountStatus::CountStatus;
};

class DeadlineMissedStatus {
 public:
  DeadlineMissedStatus() = default;
  explicit DeadlineMissedStatus(const rdds_deadline_missed_status& s) : s_(s) {}
  int32_t total_count() const { return s_.total_count; }
  int32_t total_count_change() const { return s_.total_count_change; }
  InstanceHandle last_instance_handle() const { return InstanceHandle(s_.last_instance_handle); }

 private:
  rdds_deadline_missed_status s_{};
};
class OfferedDeadlineMissedStatus : public DeadlineMissedStatus {
  using DeadlineMissedStatus::DeadlineMissedStatus;
};
class RequestedDeadlineMissedStatus : public DeadlineMissedStatus {
  using DeadlineMissedStatus::DeadlineMissedStatus;
};

class IncompatibleQosStatus {
 public:
  IncompatibleQosStatus() = default;
  explicit IncompatibleQosStatus(const rdds_incompatible_qos_status& s)
      : total_count_(s.total_count), total_count_change_(s.total_count_change), last_policy_id_(s.last_policy_id) {
    for (uint32_t i = 0; i < s.policies_len && i < RDDS_MAX_POLICIES; i++)
      policies_.emplace_back(s.policies[i].policy_id, s.policies[i].count);
  }
  int32_t total_count() const { return total_count_; }
  int32_t total_count_change() const { return total_count_change_; }
  policy::QosPolicyId last_policy_id() const { return last_policy_id_; }
  const policy::QosPolicyCountSeq& policies() const { return policies_; }

 private:
  int32_t total_count_ = 0, total_count_change_ = 0;
  policy::QosPolicyId last_policy_id_ = 0;
  policy::QosPolicyCountSeq policies_;
};
class OfferedIncompatibleQosStatus : public IncompatibleQosStatus {
  using IncompatibleQosStatus::IncompatibleQosStatus;
};
class RequestedIncompatibleQosStatus : public IncompatibleQosStatus {
  using IncompatibleQosStatus::IncompatibleQosStatus;
};

class LivelinessChangedStatus {
 public:
  LivelinessChangedStatus() = default;
  explicit LivelinessChangedStatus(const rdds_liveliness_changed_status& s) : s_(s) {}
  int32_t alive_count() const { return s_.alive_count; }
  int32_t not_alive_count() const { return s_.not_alive_count; }
  int32_t alive_count_change() const { return s_.alive_count_change; }
  int32_t not_alive_count_change() const { return s_.not_alive_count_change; }
  InstanceHandle last_publication_handle() const { return InstanceHandle(s_.last_publication_handle); }

 private:
  rdds_liveliness_changed_status s_{};
};

class SampleRejectedStatus {
 public:
  SampleRejectedStatus() = default;
  explicit SampleRejectedStatus(const rdds_sample_rejected_status& s) : s_(s) {}
  int32_t total_count() const { return s_.total_count; }
  int32_t total_count_change() const { return s_.total_count_change; }
  SampleRejectedState last_reason() const { return SampleRejectedState(s_.last_reason); }
  InstanceHandle last_instance_handle() const { return InstanceHandle(s_.last_instance_handle); }

 private:
  rdds_sample_rejected_status s_{};
};

class MatchedStatus {
 public:
  MatchedStatus() = default;
  explicit MatchedStatus(const rdds_matched_status& s) : s_(s) {}
  int32_t total_count() const { return s_.total_count; }
  int32_t total_count_change() const { return s_.total_count_change; }
  int32_t current_count() const { return s_.current_count; }
  int32_t current_count_change() const { return s_.current_count_change; }

 protected:
  rdds_matched_status s_{};
};

class PublicationMatchedStatus : public MatchedStatus {
 public:
  using MatchedStatus::MatchedStatus;
  InstanceHandle last_subscription_handle() const { return InstanceHandle(s_.last_handle); }
};

class SubscriptionMatchedStatus : public MatchedStatus {
 public:
  using MatchedStatus::MatchedStatus;
  InstanceHandle last_publication_handle() const { return InstanceHandle(s_.last_handle); }
};

}  // namespace status
}  // namespace core

// ---------------------------------------------------------------------------
// Sample, view and instance states (DDS-PSM-Cxx 1.0, 7.5.11)

namespace sub {
namespace status {

class SampleState : public std::bitset<32> {
 public:
  SampleState() : std::bitset<32>(RDDS_ANY_SAMPLE_STATE) {}
  explicit SampleState(uint32_t b) : std::bitset<32>(b) {}
  static SampleState read() { return SampleState(RDDS_READ_SAMPLE_STATE); }
  static SampleState not_read() { return SampleState(RDDS_NOT_READ_SAMPLE_STATE); }
  static SampleState any() { return SampleState(RDDS_ANY_SAMPLE_STATE); }
  uint32_t c() const { return static_cast<uint32_t>(to_ulong()); }
};

class ViewState : public std::bitset<32> {
 public:
  ViewState() : std::bitset<32>(RDDS_ANY_VIEW_STATE) {}
  explicit ViewState(uint32_t b) : std::bitset<32>(b) {}
  static ViewState new_view() { return ViewState(RDDS_NEW_VIEW_STATE); }
  static ViewState not_new_view() { return ViewState(RDDS_NOT_NEW_VIEW_STATE); }
  static ViewState any() { return ViewState(RDDS_ANY_VIEW_STATE); }
  uint32_t c() const { return static_cast<uint32_t>(to_ulong()); }
};

class InstanceState : public std::bitset<32> {
 public:
  InstanceState() : std::bitset<32>(RDDS_ANY_INSTANCE_STATE) {}
  explicit InstanceState(uint32_t b) : std::bitset<32>(b) {}
  static InstanceState alive() { return InstanceState(RDDS_ALIVE_INSTANCE_STATE); }
  static InstanceState not_alive_disposed() { return InstanceState(RDDS_NOT_ALIVE_DISPOSED_INSTANCE_STATE); }
  static InstanceState not_alive_no_writers() { return InstanceState(RDDS_NOT_ALIVE_NO_WRITERS_INSTANCE_STATE); }
  static InstanceState not_alive_mask() { return InstanceState(RDDS_NOT_ALIVE_INSTANCE_STATE); }
  static InstanceState any() { return InstanceState(RDDS_ANY_INSTANCE_STATE); }
  uint32_t c() const { return static_cast<uint32_t>(to_ulong()); }
};

class DataState {
 public:
  DataState() = default;
  DataState(const SampleState& ss) : ss_(ss) {}
  DataState(const ViewState& vs) : vs_(vs) {}
  DataState(const InstanceState& is) : is_(is) {}
  DataState(const SampleState& ss, const ViewState& vs, const InstanceState& is) : ss_(ss), vs_(vs), is_(is) {}

  static DataState any() { return DataState(); }
  static DataState new_data() { return DataState(SampleState::not_read(), ViewState::any(), InstanceState::alive()); }
  static DataState any_data() { return DataState(SampleState::any(), ViewState::any(), InstanceState::alive()); }
  static DataState new_instance() { return DataState(SampleState::any(), ViewState::new_view(), InstanceState::alive()); }

  DataState& operator<<(const SampleState& s) { ss_ = s; return *this; }
  DataState& operator<<(const ViewState& s) { vs_ = s; return *this; }
  DataState& operator<<(const InstanceState& s) { is_ = s; return *this; }
  const SampleState& sample_state() const { return ss_; }
  void sample_state(const SampleState& s) { ss_ = s; }
  const ViewState& view_state() const { return vs_; }
  void view_state(const ViewState& s) { vs_ = s; }
  const InstanceState& instance_state() const { return is_; }
  void instance_state(const InstanceState& s) { is_ = s; }
  friend bool operator==(const DataState& a, const DataState& b) {
    return a.ss_ == b.ss_ && a.vs_ == b.vs_ && a.is_ == b.is_;
  }

 private:
  SampleState ss_;
  ViewState vs_;
  InstanceState is_;
};

}  // namespace status
}  // namespace sub
}  // namespace dds

#endif  // RUSTORB_DDS_POLICY_HPP

// rustorb/dds/entities.hpp: the entities, listeners, samples, conditions
// and WaitSets of the ISO/IEC C++ PSM for DDS, on the C ABI of rustorb-dds.
// Included by <dds/dds.hpp>.
//
// Entities are references, as in the PSM: copies refer to the same entity,
// which is deleted when the last reference goes (or by close()). A child
// keeps its parents alive. An entity made from dds::core::null refers to
// nothing and compares equal to it.
//
// SPDX-License-Identifier: MIT OR Apache-2.0
#ifndef RUSTORB_DDS_ENTITIES_HPP
#define RUSTORB_DDS_ENTITIES_HPP

#include <rustorb/dds/core.hpp>
#include <rustorb/dds/policy.hpp>
#include <rustorb/dds/types.hpp>

#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <type_traits>
#include <utility>
#include <vector>

namespace dds {
namespace domain {
class DomainParticipant;
class DomainParticipantListener;
}  // namespace domain
namespace topic {
template <typename T>
class Topic;
template <typename T>
class TopicListener;
template <typename T>
class ContentFilteredTopic;
class Filter;
class AnyTopicListener;
}  // namespace topic
namespace pub {
class Publisher;
class PublisherListener;
class AnyDataWriterListener;
template <typename T>
class DataWriter;
template <typename T>
class DataWriterListener;
}  // namespace pub
namespace sub {
class Subscriber;
class SubscriberListener;
class AnyDataReaderListener;
template <typename T>
class DataReader;
template <typename T>
class DataReaderListener;
}  // namespace sub
namespace core {
namespace cond {
class Condition;
class WaitSet;
}  // namespace cond
}  // namespace core
}  // namespace dds

namespace rustorb {
namespace dds {
namespace detail {

namespace c = ::dds::core;

template <typename H, void (*Release)(H*)>
struct handle {
  H* h = nullptr;
  handle() = default;
  explicit handle(H* p) : h(p) {}
  handle(const handle&) = delete;
  handle& operator=(const handle&) = delete;
  ~handle() { reset(); }
  void reset() {
    if (h) {
      H* p = h;
      h = nullptr;
      Release(p);
    }
  }
};

inline std::vector<const char*> c_strings(const c::StringSeq& names) {
  std::vector<const char*> out;
  for (const auto& n : names) out.push_back(n.c_str());
  return out;
}

inline void to_c(const ::dds::pub::qos::DataWriterQos& q, rdds_writer_qos& c) {
  namespace p = ::dds::core::policy;
  check(rdds_writer_qos_default(&c));
  c.reliability_kind = q.policy<p::Reliability>().kind();
  c.max_blocking_time = q.policy<p::Reliability>().max_blocking_time().c();
  c.history_kind = q.policy<p::History>().kind();
  c.history_depth = q.policy<p::History>().depth();
  c.max_samples = q.policy<p::ResourceLimits>().max_samples();
  c.max_instances = q.policy<p::ResourceLimits>().max_instances();
  c.max_samples_per_instance = q.policy<p::ResourceLimits>().max_samples_per_instance();
  c.durability_kind = q.policy<p::Durability>().kind();
  c.deadline_period = q.policy<p::Deadline>().period().c();
  c.liveliness_kind = q.policy<p::Liveliness>().kind();
  c.lease_duration = q.policy<p::Liveliness>().lease_duration().c();
  const auto& ud = q.policy<p::UserData>().value();
  c.user_data = ud.data();
  c.user_data_len = ud.size();
  const auto& dr = q.policy<p::DataRepresentation>().value();
  c.data_representation = dr.data();
  c.data_representation_len = dr.size();
  c.autodispose_unregistered_instances = q.policy<p::WriterDataLifecycle>().autodispose();
}

inline void to_c(const ::dds::sub::qos::DataReaderQos& q, rdds_reader_qos& c) {
  namespace p = ::dds::core::policy;
  check(rdds_reader_qos_default(&c));
  c.reliability_kind = q.policy<p::Reliability>().kind();
  c.max_blocking_time = q.policy<p::Reliability>().max_blocking_time().c();
  c.history_kind = q.policy<p::History>().kind();
  c.history_depth = q.policy<p::History>().depth();
  c.max_samples = q.policy<p::ResourceLimits>().max_samples();
  c.max_instances = q.policy<p::ResourceLimits>().max_instances();
  c.max_samples_per_instance = q.policy<p::ResourceLimits>().max_samples_per_instance();
  c.durability_kind = q.policy<p::Durability>().kind();
  c.deadline_period = q.policy<p::Deadline>().period().c();
  c.liveliness_kind = q.policy<p::Liveliness>().kind();
  c.lease_duration = q.policy<p::Liveliness>().lease_duration().c();
  const auto& ud = q.policy<p::UserData>().value();
  c.user_data = ud.data();
  c.user_data_len = ud.size();
  const auto& dr = q.policy<p::DataRepresentation>().value();
  c.data_representation = dr.data();
  c.data_representation_len = dr.size();
  const auto& tce = q.policy<p::TypeConsistencyEnforcement>();
  c.type_consistency_kind = tce.kind();
  c.ignore_sequence_bounds = tce.ignore_sequence_bounds();
  c.ignore_string_bounds = tce.ignore_string_bounds();
  c.ignore_member_names = tce.ignore_member_names();
  c.prevent_type_widening = tce.prevent_type_widening();
  c.force_type_validation = tce.force_type_validation();
}

[[noreturn]] inline void null_reference() {
  throw c::NullReferenceError("the entity is dds::core::null");
}

[[noreturn]] inline void closed() { throw c::AlreadyClosedError("the entity is closed"); }

template <typename Impl>
Impl& deref(const std::shared_ptr<Impl>& p) {
  if (!p) null_reference();
  return *p;
}

// The listener functions of the C ABI call a listener_box, which hands
// the call to the C++ listener with a reference to the entity. Calls made
// while the entity is being created (the C ABI may call the listener
// before the create function returns) are kept and made once it has
// returned.
template <typename Impl, typename Listener>
class listener_box {
 public:
  explicit listener_box(Listener* l) : listener_(l) {}

  void call(std::function<void(const std::shared_ptr<Impl>&, Listener*)> f) {
    std::shared_ptr<Impl> impl;
    {
      std::lock_guard<std::mutex> g(m_);
      if (!ready_) {
        pending_.push_back(std::move(f));
        return;
      }
      impl = impl_.lock();
    }
    if (impl) f(impl, listener_);
  }

  void ready(const std::shared_ptr<Impl>& impl) {
    std::vector<std::function<void(const std::shared_ptr<Impl>&, Listener*)>> pending;
    {
      std::lock_guard<std::mutex> g(m_);
      impl_ = impl;
      ready_ = true;
      pending.swap(pending_);
    }
    for (auto& f : pending) f(impl, listener_);
  }

  static void release(void* self) { delete static_cast<listener_box*>(self); }

 private:
  std::mutex m_;
  bool ready_ = false;
  std::weak_ptr<Impl> impl_;
  Listener* listener_;
  std::vector<std::function<void(const std::shared_ptr<Impl>&, Listener*)>> pending_;
};

struct participant_impl {
  handle<rdds_participant, rdds_participant_release> p;
  ::dds::domain::DomainParticipantListener* listener = nullptr;
  ::dds::domain::qos::DomainParticipantQos qos;
  ::dds::topic::qos::TopicQos default_topic_qos;
  ::dds::pub::qos::PublisherQos default_publisher_qos;
  ::dds::sub::qos::SubscriberQos default_subscriber_qos;

  rdds_participant* get() const {
    if (!p.h) closed();
    return p.h;
  }
};

struct topic_impl {
  handle<rdds_topic, rdds_topic_release> t;
  void* listener = nullptr;  // a TopicListener<T> of the topic's type
  std::shared_ptr<participant_impl> participant;
  std::string name;
  std::string type_name;
  ::dds::topic::qos::TopicQos qos;

  rdds_topic* get() const {
    if (!t.h) closed();
    return t.h;
  }
};

struct cftopic_impl {
  handle<rdds_cftopic, rdds_cftopic_release> t;
  std::shared_ptr<topic_impl> topic;
  std::string name;
  std::string expression;
  std::vector<std::string> parameters;

  rdds_cftopic* get() const {
    if (!t.h) closed();
    return t.h;
  }
};

struct publisher_impl {
  handle<rdds_publisher, rdds_publisher_release> p;
  ::dds::pub::PublisherListener* listener = nullptr;
  std::shared_ptr<participant_impl> participant;
  ::dds::pub::qos::PublisherQos qos;
  ::dds::pub::qos::DataWriterQos default_writer_qos;

  rdds_publisher* get() const {
    if (!p.h) closed();
    return p.h;
  }
};

struct subscriber_impl {
  handle<rdds_subscriber, rdds_subscriber_release> s;
  ::dds::sub::SubscriberListener* listener = nullptr;
  std::shared_ptr<participant_impl> participant;
  ::dds::sub::qos::SubscriberQos qos;
  ::dds::sub::qos::DataReaderQos default_reader_qos;

  rdds_subscriber* get() const {
    if (!s.h) closed();
    return s.h;
  }
};

template <typename T>
struct writer_impl {
  handle<rdds_writer, rdds_writer_release> w;
  std::shared_ptr<publisher_impl> publisher;
  std::shared_ptr<topic_impl> topic;
  ::dds::pub::qos::DataWriterQos qos;
  ::dds::pub::DataWriterListener<T>* listener = nullptr;
  ::dds::core::status::StatusMask mask;
  // The key of each instance handle register_instance and lookup_instance
  // gave, for unregister_instance, dispose_instance and key_value, which
  // take a handle.
  std::mutex m;
  std::map<::dds::core::InstanceHandle, T> instances;

  rdds_writer* get() const {
    if (!w.h) closed();
    return w.h;
  }
};

template <typename T>
struct reader_impl {
  handle<rdds_reader, rdds_reader_release> r;
  std::shared_ptr<subscriber_impl> subscriber;
  std::shared_ptr<topic_impl> topic;
  // The content filtered topic it reads, if any.
  std::shared_ptr<cftopic_impl> cftopic;
  ::dds::sub::qos::DataReaderQos qos;
  ::dds::sub::DataReaderListener<T>* listener = nullptr;
  ::dds::core::status::StatusMask mask;
  ::dds::sub::status::DataState default_filter_state;

  rdds_reader* get() const {
    if (!r.h) closed();
    return r.h;
  }
};

struct condition_impl {
  handle<rdds_condition, rdds_condition_release> c;
  std::function<void()> handler;
  // What keeps the entity of a status or read condition alive.
  std::shared_ptr<void> owner;
};

// The C listener of a participant, publisher or subscriber hands its
// calls to the C++ listener `listener` (of type L); group_table, defined
// at the end, fills the C table with the functions L has.
template <typename L>
struct group_box {
  L* listener;
  std::weak_ptr<participant_impl> participant;
  std::weak_ptr<subscriber_impl> subscriber;

  static void release(void* self) { delete static_cast<group_box*>(self); }
};

template <typename L>
rdds_listener group_table(group_box<L>* b);

// Sets the C listener of a group entity to one that calls `l`; nullptr
// clears it.
template <typename L, typename H, typename Set>
void set_group_listener(H* h, L* l, uint32_t mask, std::weak_ptr<participant_impl> participant,
                        std::weak_ptr<subscriber_impl> subscriber, Set set) {
  if (!l) {
    check(set(h, nullptr, 0));
    return;
  }
  auto* b = new group_box<L>{l, std::move(participant), std::move(subscriber)};
  rdds_listener c = group_table(b);
  check(set(h, &c, mask));
}

}  // namespace detail

// rustorb: a participant that uses only the loopback interface, for tests.
inline ::dds::domain::DomainParticipant localhost_participant(uint32_t domain_id);

}  // namespace dds
}  // namespace rustorb

namespace dds {

// ---------------------------------------------------------------------------
// Listeners of groups (DDS 1.4, 2.2.4.3). A status of a writer or reader
// that its own listener does not take goes to the listener of its
// publisher or subscriber, else to the participant's; INCONSISTENT_TOPIC
// to the topic's, else the participant's. New data goes first to
// on_data_on_readers (subscriber, else participant), and only when neither
// takes DATA_ON_READERS to on_data_available.

namespace pub {

// A writer of any type, as given to the listener of its publisher or
// participant.
class AnyDataWriter {
 public:
  explicit AnyDataWriter(const rdds_endpoint_info& i)
      : handle_(i.guid), topic_name_(i.topic_name), type_name_(i.type_name) {}
  const std::string& topic_name() const { return topic_name_; }
  const std::string& type_name() const { return type_name_; }
  const ::dds::core::InstanceHandle& instance_handle() const { return handle_; }

 private:
  ::dds::core::InstanceHandle handle_;
  std::string topic_name_;
  std::string type_name_;
};

class AnyDataWriterListener {
 public:
  virtual ~AnyDataWriterListener() = default;
  virtual void on_offered_deadline_missed(AnyDataWriter& writer,
                                          const ::dds::core::status::OfferedDeadlineMissedStatus& status) = 0;
  virtual void on_offered_incompatible_qos(AnyDataWriter& writer,
                                           const ::dds::core::status::OfferedIncompatibleQosStatus& status) = 0;
  virtual void on_liveliness_lost(AnyDataWriter& writer, const ::dds::core::status::LivelinessLostStatus& status) = 0;
  virtual void on_publication_matched(AnyDataWriter& writer,
                                      const ::dds::core::status::PublicationMatchedStatus& status) = 0;
};

class NoOpAnyDataWriterListener : public virtual AnyDataWriterListener {
 public:
  void on_offered_deadline_missed(AnyDataWriter&, const ::dds::core::status::OfferedDeadlineMissedStatus&) override {}
  void on_offered_incompatible_qos(AnyDataWriter&,
                                   const ::dds::core::status::OfferedIncompatibleQosStatus&) override {}
  void on_liveliness_lost(AnyDataWriter&, const ::dds::core::status::LivelinessLostStatus&) override {}
  void on_publication_matched(AnyDataWriter&, const ::dds::core::status::PublicationMatchedStatus&) override {}
};

class PublisherListener : public virtual AnyDataWriterListener {};
class NoOpPublisherListener : public virtual PublisherListener, public virtual NoOpAnyDataWriterListener {};

}  // namespace pub

namespace sub {

// A reader of any type, as given to the listener of its subscriber or
// participant.
class AnyDataReader {
 public:
  explicit AnyDataReader(const rdds_endpoint_info& i)
      : handle_(i.guid), topic_name_(i.topic_name), type_name_(i.type_name) {}
  const std::string& topic_name() const { return topic_name_; }
  const std::string& type_name() const { return type_name_; }
  const ::dds::core::InstanceHandle& instance_handle() const { return handle_; }

 private:
  ::dds::core::InstanceHandle handle_;
  std::string topic_name_;
  std::string type_name_;
};

class AnyDataReaderListener {
 public:
  virtual ~AnyDataReaderListener() = default;
  virtual void on_requested_deadline_missed(AnyDataReader& reader,
                                            const ::dds::core::status::RequestedDeadlineMissedStatus& status) = 0;
  virtual void on_requested_incompatible_qos(AnyDataReader& reader,
                                             const ::dds::core::status::RequestedIncompatibleQosStatus& status) = 0;
  virtual void on_sample_rejected(AnyDataReader& reader, const ::dds::core::status::SampleRejectedStatus& status) = 0;
  virtual void on_liveliness_changed(AnyDataReader& reader,
                                     const ::dds::core::status::LivelinessChangedStatus& status) = 0;
  virtual void on_data_available(AnyDataReader& reader) = 0;
  virtual void on_subscription_matched(AnyDataReader& reader,
                                       const ::dds::core::status::SubscriptionMatchedStatus& status) = 0;
  virtual void on_sample_lost(AnyDataReader& reader, const ::dds::core::status::SampleLostStatus& status) = 0;
};

class NoOpAnyDataReaderListener : public virtual AnyDataReaderListener {
 public:
  void on_requested_deadline_missed(AnyDataReader&,
                                    const ::dds::core::status::RequestedDeadlineMissedStatus&) override {}
  void on_requested_incompatible_qos(AnyDataReader&,
                                     const ::dds::core::status::RequestedIncompatibleQosStatus&) override {}
  void on_sample_rejected(AnyDataReader&, const ::dds::core::status::SampleRejectedStatus&) override {}
  void on_liveliness_changed(AnyDataReader&, const ::dds::core::status::LivelinessChangedStatus&) override {}
  void on_data_available(AnyDataReader&) override {}
  void on_subscription_matched(AnyDataReader&, const ::dds::core::status::SubscriptionMatchedStatus&) override {}
  void on_sample_lost(AnyDataReader&, const ::dds::core::status::SampleLostStatus&) override {}
};

// The subscriber given to on_data_on_readers is a handle of the
// subscriber; one made from a participant's listener has the default QoS
// in qos().
class SubscriberListener : public virtual AnyDataReaderListener {
 public:
  virtual void on_data_on_readers(Subscriber& sub) = 0;
};
class NoOpSubscriberListener : public virtual SubscriberListener, public virtual NoOpAnyDataReaderListener {
 public:
  void on_data_on_readers(Subscriber&) override {}
};

}  // namespace sub

namespace topic {

// A topic of any type, as given to the participant's listener.
class AnyTopic {
 public:
  explicit AnyTopic(const char* name) : name_(name) {}
  const std::string& name() const { return name_; }

 private:
  std::string name_;
};

class AnyTopicListener {
 public:
  virtual ~AnyTopicListener() = default;
  virtual void on_inconsistent_topic(AnyTopic& topic, const ::dds::core::status::InconsistentTopicStatus& status) = 0;
};
class NoOpAnyTopicListener : public virtual AnyTopicListener {
 public:
  void on_inconsistent_topic(AnyTopic&, const ::dds::core::status::InconsistentTopicStatus&) override {}
};

}  // namespace topic

// ---------------------------------------------------------------------------
// Domain

namespace domain {

class DomainParticipantListener : public virtual ::dds::pub::PublisherListener,
                                  public virtual ::dds::sub::SubscriberListener,
                                  public virtual ::dds::topic::AnyTopicListener {};
class NoOpDomainParticipantListener : public virtual DomainParticipantListener,
                                      public virtual ::dds::pub::NoOpPublisherListener,
                                      public virtual ::dds::sub::NoOpSubscriberListener,
                                      public virtual ::dds::topic::NoOpAnyTopicListener {};

class DomainParticipant {
 public:
  DomainParticipant(const ::dds::core::null_type&) {}
  explicit DomainParticipant(uint32_t domain_id)
      : DomainParticipant(domain_id, qos::DomainParticipantQos()) {}
  DomainParticipant(uint32_t domain_id, const qos::DomainParticipantQos& qos,
                    DomainParticipantListener* listener = nullptr,
                    const ::dds::core::status::StatusMask& mask = ::dds::core::status::StatusMask::none())
      : DomainParticipant(domain_id, qos, listener, mask, 0) {}

  uint32_t domain_id() const { return rdds_participant_domain_id(impl().get()); }
  void assert_liveliness() { rustorb::dds::detail::check(rdds_participant_assert_liveliness(impl().get())); }
  ::dds::core::Time current_time() const { return ::dds::core::Time::now(); }
  const qos::DomainParticipantQos& qos() const { return impl().qos; }
  ::dds::core::InstanceHandle instance_handle() const {
    rdds_guid g;
    rustorb::dds::detail::check(rdds_participant_guid(impl().get(), &g));
    return ::dds::core::InstanceHandle(g);
  }
  // rustorb: how many other participants are known.
  size_t discovered_participants() const { return rdds_participant_discovered_participants(impl().get()); }

  const ::dds::topic::qos::TopicQos& default_topic_qos() const { return impl().default_topic_qos; }
  DomainParticipant& default_topic_qos(const ::dds::topic::qos::TopicQos& q) {
    impl().default_topic_qos = q;
    return *this;
  }
  const ::dds::pub::qos::PublisherQos& default_publisher_qos() const { return impl().default_publisher_qos; }
  DomainParticipant& default_publisher_qos(const ::dds::pub::qos::PublisherQos& q) {
    impl().default_publisher_qos = q;
    return *this;
  }
  const ::dds::sub::qos::SubscriberQos& default_subscriber_qos() const { return impl().default_subscriber_qos; }
  DomainParticipant& default_subscriber_qos(const ::dds::sub::qos::SubscriberQos& q) {
    impl().default_subscriber_qos = q;
    return *this;
  }

  // Sets the listener (nullptr clears it); the caller keeps it alive.
  void listener(DomainParticipantListener* l, const ::dds::core::status::StatusMask& mask) {
    rustorb::dds::detail::set_group_listener(impl().get(), l, mask.c(), impl_, {}, rdds_participant_set_listener);
    impl().listener = l;
  }
  DomainParticipantListener* listener() const { return impl().listener; }

  void enable() {}
  void close() { impl().p.reset(); }
  void retain() {}

  friend bool operator==(const DomainParticipant& a, const DomainParticipant& b) { return a.impl_ == b.impl_; }
  friend bool operator!=(const DomainParticipant& a, const DomainParticipant& b) { return a.impl_ != b.impl_; }
  friend bool operator==(const DomainParticipant& a, const ::dds::core::null_type&) { return !a.impl_; }
  friend bool operator!=(const DomainParticipant& a, const ::dds::core::null_type&) { return !!a.impl_; }

  // rustorb
  const std::shared_ptr<rustorb::dds::detail::participant_impl>& rustorb_impl() const { return impl_; }
  static DomainParticipant rustorb_from(std::shared_ptr<rustorb::dds::detail::participant_impl> impl) {
    DomainParticipant p(::dds::core::null);
    p.impl_ = std::move(impl);
    return p;
  }

 private:
  friend DomainParticipant rustorb::dds::localhost_participant(uint32_t);

  DomainParticipant(uint32_t domain_id, const qos::DomainParticipantQos& qos, DomainParticipantListener* listener,
                    const ::dds::core::status::StatusMask& mask, uint32_t flags) {
    auto impl = std::make_shared<rustorb::dds::detail::participant_impl>();
    impl->qos = qos;
    const auto& ud = qos.policy<::dds::core::policy::UserData>().value();
    rdds_participant_qos c{ud.data(), ud.size()};
    rdds_participant* p = nullptr;
    rustorb::dds::detail::check(rdds_participant_new(domain_id, &c, flags, &p));
    impl->p.h = p;
    impl_ = impl;
    if (listener) this->listener(listener, mask);
  }

  rustorb::dds::detail::participant_impl& impl() const { return rustorb::dds::detail::deref(impl_); }

  std::shared_ptr<rustorb::dds::detail::participant_impl> impl_;
};

}  // namespace domain

// ---------------------------------------------------------------------------
// Topics

namespace topic {

template <typename T>
class TopicListener {
 public:
  virtual ~TopicListener() = default;
  virtual void on_inconsistent_topic(Topic<T>& topic, const ::dds::core::status::InconsistentTopicStatus& status) = 0;
};
template <typename T>
class NoOpTopicListener : public virtual TopicListener<T> {
 public:
  void on_inconsistent_topic(Topic<T>&, const ::dds::core::status::InconsistentTopicStatus&) override {}
};

template <typename T>
class Topic {
  static_assert(is_topic_type<T>::value,
                "T is not a topic type: include the Foo_dds.hh rustorb-idl wrote for it");

 public:
  using DataType = T;

  Topic(const ::dds::core::null_type&) {}
  Topic(const ::dds::domain::DomainParticipant& dp, const std::string& name)
      : Topic(dp, name, dp.default_topic_qos()) {}
  Topic(const ::dds::domain::DomainParticipant& dp, const std::string& name, const qos::TopicQos& qos,
        TopicListener<T>* listener = nullptr,
        const ::dds::core::status::StatusMask& mask = ::dds::core::status::StatusMask::none()) {
    auto impl = std::make_shared<rustorb::dds::detail::topic_impl>();
    impl->participant = dp.rustorb_impl();
    impl->name = name;
    impl->type_name = topic_type_name<T>::value();
    impl->qos = qos;
    rdds_topic* t = nullptr;
    rustorb::dds::detail::check(rdds_participant_create_topic(rustorb::dds::detail::deref(impl->participant).get(),
                                                              name.c_str(), rustorb::dds::codec<T>::type(), &t));
    impl->t.h = t;
    impl_ = impl;
    if (listener) this->listener(listener, mask);
  }

  // Sets the listener (nullptr clears it); the caller keeps it alive. The
  // topics of one name in a participant share their listener.
  void listener(TopicListener<T>* l, const ::dds::core::status::StatusMask& mask) {
    if (!l) {
      rustorb::dds::detail::check(rdds_topic_set_listener(impl().get(), nullptr, 0));
    } else {
      struct box {
        TopicListener<T>* l;
        std::weak_ptr<rustorb::dds::detail::topic_impl> topic;
      };
      rdds_listener c{};
      c.self = new box{l, impl_};
      c.on_inconsistent_topic = [](void* self, const char*, const rdds_count_status* s) {
        auto* b = static_cast<box*>(self);
        if (auto i = b->topic.lock()) {
          Topic t = Topic::rustorb_from(i);
          b->l->on_inconsistent_topic(t, ::dds::core::status::InconsistentTopicStatus(*s));
        }
      };
      c.release = [](void* self) { delete static_cast<box*>(self); };
      rustorb::dds::detail::check(rdds_topic_set_listener(impl().get(), &c, mask.c()));
    }
    impl().listener = l;
  }
  TopicListener<T>* listener() const { return static_cast<TopicListener<T>*>(impl().listener); }

  const std::string& name() const { return impl().name; }
  const std::string& type_name() const { return impl().type_name; }
  ::dds::domain::DomainParticipant domain_participant() const;
  const qos::TopicQos& qos() const { return impl().qos; }
  void qos(const qos::TopicQos& q) { impl().qos = q; }
  ::dds::core::status::InconsistentTopicStatus inconsistent_topic_status() {
    rdds_count_status s;
    rustorb::dds::detail::check(rdds_topic_inconsistent_topic_status(impl().get(), &s));
    return ::dds::core::status::InconsistentTopicStatus(s);
  }

  void enable() {}
  void close() { impl().t.reset(); }
  void retain() {}

  friend bool operator==(const Topic& a, const Topic& b) { return a.impl_ == b.impl_; }
  friend bool operator!=(const Topic& a, const Topic& b) { return a.impl_ != b.impl_; }
  friend bool operator==(const Topic& a, const ::dds::core::null_type&) { return !a.impl_; }
  friend bool operator!=(const Topic& a, const ::dds::core::null_type&) { return !!a.impl_; }

  // rustorb
  const std::shared_ptr<rustorb::dds::detail::topic_impl>& rustorb_impl() const { return impl_; }
  static Topic rustorb_from(std::shared_ptr<rustorb::dds::detail::topic_impl> impl) {
    Topic t(::dds::core::null);
    t.impl_ = std::move(impl);
    return t;
  }

 private:
  rustorb::dds::detail::topic_impl& impl() const { return rustorb::dds::detail::deref(impl_); }
  std::shared_ptr<rustorb::dds::detail::topic_impl> impl_;
};

// A filter expression (DDS 1.4, Annex B) and its parameters, for a
// ContentFilteredTopic.
class Filter {
 public:
  using const_iterator = std::vector<std::string>::const_iterator;

  Filter(const std::string& query_expression) : expression_(query_expression) {}
  template <typename FWIterator>
  Filter(const std::string& query_expression, const FWIterator& params_begin, const FWIterator& params_end)
      : expression_(query_expression), parameters_(params_begin, params_end) {}
  Filter(const std::string& query_expression, const std::vector<std::string>& params)
      : expression_(query_expression), parameters_(params) {}

  const std::string& expression() const { return expression_; }
  const_iterator begin() const { return parameters_.begin(); }
  const_iterator end() const { return parameters_.end(); }
  template <typename FWIterator>
  void parameters(const FWIterator& begin, const FWIterator& end) {
    parameters_.assign(begin, end);
  }
  void add_parameter(const std::string& param) { parameters_.push_back(param); }
  uint32_t parameters_length() const { return static_cast<uint32_t>(parameters_.size()); }
  // rustorb
  const std::vector<std::string>& rustorb_parameters() const { return parameters_; }

 private:
  std::string expression_;
  std::vector<std::string> parameters_;
};

// A topic whose readers get only the samples of its related topic that
// pass a filter (DDS 1.4, 2.2.2.3.3). The type must have been generated
// with -Wbxtypes, so that its fields can be read by name.
template <typename T>
class ContentFilteredTopic {
 public:
  using DataType = T;

  ContentFilteredTopic(const ::dds::core::null_type&) {}
  ContentFilteredTopic(const Topic<T>& topic, const std::string& name, const Filter& filter)
      : topic_(topic) {
    auto impl = std::make_shared<rustorb::dds::detail::cftopic_impl>();
    impl->topic = topic.rustorb_impl();
    impl->name = name;
    impl->expression = filter.expression();
    impl->parameters = filter.rustorb_parameters();
    auto params = rustorb::dds::detail::c_strings(impl->parameters);
    rdds_cftopic* t = nullptr;
    rustorb::dds::detail::check(rdds_participant_create_contentfilteredtopic(
        rustorb::dds::detail::deref(impl->topic->participant).get(), name.c_str(),
        rustorb::dds::detail::deref(impl->topic).get(), impl->expression.c_str(), params.data(), params.size(), &t));
    impl->t.h = t;
    impl_ = impl;
  }

  const std::string& name() const { return impl().name; }
  const std::string& type_name() const { return topic_.type_name(); }
  const Topic<T>& topic() const { return topic_; }
  const std::string& filter_expression() const { return impl().expression; }
  const std::vector<std::string>& filter_parameters() const { return impl().parameters; }
  Filter filter() const { return Filter(impl().expression, impl().parameters); }
  template <typename FWIterator>
  void filter_parameters(const FWIterator& begin, const FWIterator& end) {
    std::vector<std::string> p(begin, end);
    auto params = rustorb::dds::detail::c_strings(p);
    rustorb::dds::detail::check(rdds_cftopic_set_expression_parameters(impl().get(), params.data(), params.size()));
    impl().parameters = std::move(p);
  }
  void close() { impl().t.reset(); }

  friend bool operator==(const ContentFilteredTopic& a, const ContentFilteredTopic& b) { return a.impl_ == b.impl_; }
  friend bool operator!=(const ContentFilteredTopic& a, const ContentFilteredTopic& b) { return a.impl_ != b.impl_; }

  // rustorb
  const std::shared_ptr<rustorb::dds::detail::cftopic_impl>& rustorb_impl() const { return impl_; }

 private:
  rustorb::dds::detail::cftopic_impl& impl() const { return rustorb::dds::detail::deref(impl_); }
  Topic<T> topic_ = Topic<T>(::dds::core::null);
  std::shared_ptr<rustorb::dds::detail::cftopic_impl> impl_;
};

}  // namespace topic

namespace domain {
// rustorb: a reference to the participant of an implementation object.
inline DomainParticipant rustorb_participant(std::shared_ptr<rustorb::dds::detail::participant_impl> impl);
}  // namespace domain

// ---------------------------------------------------------------------------
// Publication

namespace pub {

class Publisher {
 public:
  Publisher(const ::dds::core::null_type&) {}
  explicit Publisher(const ::dds::domain::DomainParticipant& dp) : Publisher(dp, dp.default_publisher_qos()) {}
  Publisher(const ::dds::domain::DomainParticipant& dp, const qos::PublisherQos& qos,
            PublisherListener* listener = nullptr,
            const ::dds::core::status::StatusMask& mask = ::dds::core::status::StatusMask::none()) {
    auto impl = std::make_shared<rustorb::dds::detail::publisher_impl>();
    impl->participant = dp.rustorb_impl();
    impl->qos = qos;
    auto names = rustorb::dds::detail::c_strings(qos.policy<::dds::core::policy::Partition>().name());
    rdds_group_qos c{names.data(), names.size()};
    rdds_publisher* p = nullptr;
    rustorb::dds::detail::check(
        rdds_participant_create_publisher(rustorb::dds::detail::deref(impl->participant).get(), &c, &p));
    impl->p.h = p;
    impl_ = impl;
    if (listener) this->listener(listener, mask);
  }

  // Sets the listener (nullptr clears it); the caller keeps it alive.
  void listener(PublisherListener* l, const ::dds::core::status::StatusMask& mask) {
    rustorb::dds::detail::set_group_listener(impl().get(), l, mask.c(), impl().participant, {},
                                             rdds_publisher_set_listener);
    impl().listener = l;
  }
  PublisherListener* listener() const { return impl().listener; }

  const qos::PublisherQos& qos() const { return impl().qos; }
  const qos::DataWriterQos& default_datawriter_qos() const { return impl().default_writer_qos; }
  Publisher& default_datawriter_qos(const qos::DataWriterQos& q) {
    impl().default_writer_qos = q;
    return *this;
  }
  ::dds::domain::DomainParticipant participant() const {
    return ::dds::domain::rustorb_participant(impl().participant);
  }

  void enable() {}
  void close() { impl().p.reset(); }
  void retain() {}

  friend bool operator==(const Publisher& a, const Publisher& b) { return a.impl_ == b.impl_; }
  friend bool operator!=(const Publisher& a, const Publisher& b) { return a.impl_ != b.impl_; }
  friend bool operator==(const Publisher& a, const ::dds::core::null_type&) { return !a.impl_; }
  friend bool operator!=(const Publisher& a, const ::dds::core::null_type&) { return !!a.impl_; }

  // rustorb
  const std::shared_ptr<rustorb::dds::detail::publisher_impl>& rustorb_impl() const { return impl_; }
  static Publisher rustorb_from(std::shared_ptr<rustorb::dds::detail::publisher_impl> impl) {
    Publisher p(::dds::core::null);
    p.impl_ = std::move(impl);
    return p;
  }

 private:
  rustorb::dds::detail::publisher_impl& impl() const { return rustorb::dds::detail::deref(impl_); }
  std::shared_ptr<rustorb::dds::detail::publisher_impl> impl_;
};

template <typename T>
class DataWriterListener {
 public:
  virtual ~DataWriterListener() = default;
  virtual void on_offered_deadline_missed(DataWriter<T>& writer,
                                          const ::dds::core::status::OfferedDeadlineMissedStatus& status) = 0;
  virtual void on_offered_incompatible_qos(DataWriter<T>& writer,
                                           const ::dds::core::status::OfferedIncompatibleQosStatus& status) = 0;
  virtual void on_liveliness_lost(DataWriter<T>& writer, const ::dds::core::status::LivelinessLostStatus& status) = 0;
  virtual void on_publication_matched(DataWriter<T>& writer,
                                      const ::dds::core::status::PublicationMatchedStatus& status) = 0;
};

template <typename T>
class NoOpDataWriterListener : public virtual DataWriterListener<T> {
 public:
  void on_offered_deadline_missed(DataWriter<T>&, const ::dds::core::status::OfferedDeadlineMissedStatus&) override {}
  void on_offered_incompatible_qos(DataWriter<T>&,
                                   const ::dds::core::status::OfferedIncompatibleQosStatus&) override {}
  void on_liveliness_lost(DataWriter<T>&, const ::dds::core::status::LivelinessLostStatus&) override {}
  void on_publication_matched(DataWriter<T>&, const ::dds::core::status::PublicationMatchedStatus&) override {}
};

template <typename T>
class DataWriter {
  using impl_type = rustorb::dds::detail::writer_impl<T>;
  using box = rustorb::dds::detail::listener_box<impl_type, DataWriterListener<T>>;

 public:
  DataWriter(const ::dds::core::null_type&) {}
  DataWriter(const Publisher& pub, const ::dds::topic::Topic<T>& topic)
      : DataWriter(pub, topic, pub.default_datawriter_qos()) {}
  DataWriter(const Publisher& pub, const ::dds::topic::Topic<T>& topic, const qos::DataWriterQos& qos,
             DataWriterListener<T>* listener = nullptr,
             const ::dds::core::status::StatusMask& mask = ::dds::core::status::StatusMask::all()) {
    auto impl = std::make_shared<impl_type>();
    impl->publisher = pub.rustorb_impl();
    impl->topic = topic.rustorb_impl();
    impl->qos = qos;
    impl->listener = listener;
    impl->mask = mask;
    rdds_writer_qos c;
    rustorb::dds::detail::to_c(qos, c);
    box* b = listener ? new box(listener) : nullptr;
    rdds_writer_listener l = table(b);
    rdds_writer* w = nullptr;
    rdds_status st = rdds_publisher_create_writer(rustorb::dds::detail::deref(impl->publisher).get(),
                                                  rustorb::dds::detail::deref(impl->topic).get(), &c,
                                                  b ? &l : nullptr, mask.c(), &w);
    if (st != RDDS_RETCODE_OK) {
      // The library released the listener (and the box) or never took it.
      rustorb::dds::detail::raise(st);
    }
    impl->w.h = w;
    impl_ = impl;
    if (b) b->ready(impl_);
  }

  void write(const T& sample) { write_c(sample, nullptr); }
  void write(const T& sample, const ::dds::core::Time& timestamp) {
    rdds_time t = timestamp.c();
    write_c(sample, &t);
  }
  void write(const T& sample, const ::dds::core::InstanceHandle&) { write(sample); }
  void write(const T& sample, const ::dds::core::InstanceHandle&, const ::dds::core::Time& timestamp) {
    write(sample, timestamp);
  }
  template <typename FWIterator>
  void write(const FWIterator& begin, const FWIterator& end) {
    for (FWIterator i = begin; i != end; ++i) write(*i);
  }
  DataWriter& operator<<(const T& sample) {
    write(sample);
    return *this;
  }
  DataWriter& operator<<(const std::pair<T, ::dds::core::Time>& sample) {
    write(sample.first, sample.second);
    return *this;
  }
  DataWriter& operator<<(const std::pair<T, ::dds::core::InstanceHandle>& sample) {
    write(sample.first);
    return *this;
  }

  const ::dds::core::InstanceHandle register_instance(const T& key) {
    rustorb::dds::out_stream s;
    rustorb::dds::write_sample(s, key);
    rdds_instance_handle h;
    rustorb::dds::detail::check(rdds_writer_register_instance(impl().get(), s.get(), &h));
    ::dds::core::InstanceHandle ih(h);
    remember(ih, key);
    return ih;
  }
  const ::dds::core::InstanceHandle register_instance(const T& key, const ::dds::core::Time&) {
    return register_instance(key);
  }

  // The handle must be one register_instance or lookup_instance gave.
  DataWriter& unregister_instance(const ::dds::core::InstanceHandle& i) {
    return unregister_instance(i, ::dds::core::Time::invalid());
  }
  DataWriter& unregister_instance(const ::dds::core::InstanceHandle& i, const ::dds::core::Time& timestamp) {
    T key = known(i);
    unregister_c(key, timestamp);
    forget(i);
    return *this;
  }
  // rustorb: unregisters the instance of the key fields of a sample.
  DataWriter& unregister_instance(const T& key) {
    unregister_c(key, ::dds::core::Time::invalid());
    return *this;
  }

  // The handle must be one register_instance or lookup_instance gave.
  DataWriter& dispose_instance(const ::dds::core::InstanceHandle& i) {
    return dispose_instance(i, ::dds::core::Time::invalid());
  }
  DataWriter& dispose_instance(const ::dds::core::InstanceHandle& i, const ::dds::core::Time& timestamp) {
    dispose_c(known(i), timestamp);
    return *this;
  }
  // rustorb: disposes of the instance of the key fields of a sample.
  DataWriter& dispose_instance(const T& key) {
    dispose_c(key, ::dds::core::Time::invalid());
    return *this;
  }

  T& key_value(T& sample, const ::dds::core::InstanceHandle& h) { return sample = known(h); }

  ::dds::core::InstanceHandle lookup_instance(const T& key) {
    rustorb::dds::out_stream s;
    rustorb::dds::write_sample(s, key);
    rdds_instance_handle h;
    rustorb::dds::detail::check(rdds_writer_lookup_instance(impl().get(), s.get(), &h));
    ::dds::core::InstanceHandle ih(h);
    if (!ih.is_nil()) remember(ih, key);
    return ih;
  }

  const qos::DataWriterQos& qos() const { return impl().qos; }
  void qos(const qos::DataWriterQos&) {
    throw ::dds::core::UnsupportedError("the QoS of a DataWriter cannot be changed yet");
  }
  const ::dds::topic::Topic<T> topic() const { return ::dds::topic::Topic<T>::rustorb_from(impl().topic); }
  const Publisher publisher() const { return Publisher::rustorb_from(impl().publisher); }

  void wait_for_acknowledgments(const ::dds::core::Duration& timeout) {
    rustorb::dds::detail::check(rdds_writer_wait_for_acknowledgments(impl().get(), timeout.c()));
  }
  // rustorb: until count readers are matched; false on timeout.
  bool wait_for_subscriptions(size_t count, const ::dds::core::Duration& timeout) {
    rdds_status st = rdds_writer_wait_for_subscriptions(impl().get(), count, timeout.c());
    if (st == RDDS_RETCODE_TIMEOUT) return false;
    rustorb::dds::detail::check(st);
    return true;
  }
  ::dds::core::InstanceHandleSeq matched_subscriptions() const {
    std::vector<rdds_guid> buf(16);
    size_t len = 0;
    for (;;) {
      rustorb::dds::detail::check(rdds_writer_matched_subscriptions(impl().get(), buf.data(), buf.size(), &len));
      if (len <= buf.size()) break;
      buf.resize(len);
    }
    ::dds::core::InstanceHandleSeq out;
    for (size_t i = 0; i < len; i++) out.emplace_back(buf[i]);
    return out;
  }

  void listener(DataWriterListener<T>* l, const ::dds::core::status::StatusMask& mask) {
    box* b = l ? new box(l) : nullptr;
    if (b) b->ready(impl_);
    rdds_writer_listener t = table(b);
    rustorb::dds::detail::check(rdds_writer_set_listener(impl().get(), b ? &t : nullptr, mask.c()));
    impl().listener = l;
    impl().mask = mask;
  }
  DataWriterListener<T>* listener() const { return impl().listener; }

  const ::dds::core::status::LivelinessLostStatus liveliness_lost_status() {
    rdds_count_status s;
    rustorb::dds::detail::check(rdds_writer_liveliness_lost_status(impl().get(), &s));
    return ::dds::core::status::LivelinessLostStatus(s);
  }
  const ::dds::core::status::OfferedDeadlineMissedStatus offered_deadline_missed_status() {
    rdds_deadline_missed_status s;
    rustorb::dds::detail::check(rdds_writer_offered_deadline_missed_status(impl().get(), &s));
    return ::dds::core::status::OfferedDeadlineMissedStatus(s);
  }
  const ::dds::core::status::OfferedIncompatibleQosStatus offered_incompatible_qos_status() {
    rdds_incompatible_qos_status s;
    rustorb::dds::detail::check(rdds_writer_offered_incompatible_qos_status(impl().get(), &s));
    return ::dds::core::status::OfferedIncompatibleQosStatus(s);
  }
  const ::dds::core::status::PublicationMatchedStatus publication_matched_status() {
    rdds_matched_status s;
    rustorb::dds::detail::check(rdds_writer_publication_matched_status(impl().get(), &s));
    return ::dds::core::status::PublicationMatchedStatus(s);
  }
  // The names of DDS-PSM-Cxx 1.0, as it spells them.
  const ::dds::core::status::OfferedDeadlineMissedStatus offered_deadlined_missed_status() {
    return offered_deadline_missed_status();
  }
  const ::dds::core::status::OfferedIncompatibleQosStatus offered_incompatible_qos_staus() {
    return offered_incompatible_qos_status();
  }

  void assert_liveliness() { rustorb::dds::detail::check(rdds_writer_assert_liveliness(impl().get())); }
  ::dds::core::status::StatusMask status_changes() const {
    return ::dds::core::status::StatusMask(rdds_writer_status_changes(impl().get()));
  }
  ::dds::core::InstanceHandle instance_handle() const {
    rdds_guid g;
    rustorb::dds::detail::check(rdds_writer_guid(impl().get(), &g));
    return ::dds::core::InstanceHandle(g);
  }

  void enable() {}
  void close() { impl().w.reset(); }
  void retain() {}

  friend bool operator==(const DataWriter& a, const DataWriter& b) { return a.impl_ == b.impl_; }
  friend bool operator!=(const DataWriter& a, const DataWriter& b) { return a.impl_ != b.impl_; }
  friend bool operator==(const DataWriter& a, const ::dds::core::null_type&) { return !a.impl_; }
  friend bool operator!=(const DataWriter& a, const ::dds::core::null_type&) { return !!a.impl_; }

  // rustorb
  rdds_condition* rustorb_status_condition() const {
    rdds_condition* c = nullptr;
    rustorb::dds::detail::check(rdds_writer_status_condition(impl().get(), &c));
    return c;
  }
  std::shared_ptr<void> rustorb_owner() const { return impl_; }

 private:
  impl_type& impl() const { return rustorb::dds::detail::deref(impl_); }

  explicit DataWriter(std::shared_ptr<impl_type> impl) : impl_(std::move(impl)) {}

  void write_c(const T& sample, const rdds_time* t) {
    rustorb::dds::out_stream s;
    rustorb::dds::write_sample(s, sample);
    rustorb::dds::detail::check(rdds_writer_write(impl().get(), s.get(), t));
  }
  void unregister_c(const T& key, const ::dds::core::Time& timestamp) {
    rustorb::dds::out_stream s;
    rustorb::dds::write_sample(s, key);
    rdds_time t = timestamp.c();
    rustorb::dds::detail::check(
        rdds_writer_unregister_instance(impl().get(), s.get(), timestamp.is_invalid() ? nullptr : &t));
  }
  void dispose_c(const T& key, const ::dds::core::Time& timestamp) {
    rustorb::dds::out_stream s;
    rustorb::dds::write_sample(s, key);
    rdds_time t = timestamp.c();
    rustorb::dds::detail::check(rdds_writer_dispose(impl().get(), s.get(), timestamp.is_invalid() ? nullptr : &t));
  }
  void remember(const ::dds::core::InstanceHandle& h, const T& key) {
    std::lock_guard<std::mutex> g(impl().m);
    impl().instances[h] = key;
  }
  void forget(const ::dds::core::InstanceHandle& h) {
    std::lock_guard<std::mutex> g(impl().m);
    impl().instances.erase(h);
  }
  T known(const ::dds::core::InstanceHandle& h) {
    std::lock_guard<std::mutex> g(impl().m);
    auto i = impl().instances.find(h);
    if (i == impl().instances.end())
      throw ::dds::core::PreconditionNotMetError(
          "the instance handle did not come from register_instance or lookup_instance of this writer");
    return i->second;
  }

  static rdds_writer_listener table(box* b) {
    rdds_writer_listener l{};
    if (!b) return l;
    l.self = b;
    l.on_offered_deadline_missed = [](void* self, const rdds_deadline_missed_status* s) {
      ::dds::core::status::OfferedDeadlineMissedStatus st(*s);
      static_cast<box*>(self)->call([st](const std::shared_ptr<impl_type>& i, DataWriterListener<T>* l) {
        DataWriter w(i);
        l->on_offered_deadline_missed(w, st);
      });
    };
    l.on_offered_incompatible_qos = [](void* self, const rdds_incompatible_qos_status* s) {
      ::dds::core::status::OfferedIncompatibleQosStatus st(*s);
      static_cast<box*>(self)->call([st](const std::shared_ptr<impl_type>& i, DataWriterListener<T>* l) {
        DataWriter w(i);
        l->on_offered_incompatible_qos(w, st);
      });
    };
    l.on_liveliness_lost = [](void* self, const rdds_count_status* s) {
      ::dds::core::status::LivelinessLostStatus st(*s);
      static_cast<box*>(self)->call([st](const std::shared_ptr<impl_type>& i, DataWriterListener<T>* l) {
        DataWriter w(i);
        l->on_liveliness_lost(w, st);
      });
    };
    l.on_publication_matched = [](void* self, const rdds_matched_status* s) {
      ::dds::core::status::PublicationMatchedStatus st(*s);
      static_cast<box*>(self)->call([st](const std::shared_ptr<impl_type>& i, DataWriterListener<T>* l) {
        DataWriter w(i);
        l->on_publication_matched(w, st);
      });
    };
    l.release = &box::release;
    return l;
  }

  std::shared_ptr<impl_type> impl_;
};

}  // namespace pub

// ---------------------------------------------------------------------------
// Subscription

namespace sub {

class Subscriber {
 public:
  Subscriber(const ::dds::core::null_type&) {}
  explicit Subscriber(const ::dds::domain::DomainParticipant& dp) : Subscriber(dp, dp.default_subscriber_qos()) {}
  Subscriber(const ::dds::domain::DomainParticipant& dp, const qos::SubscriberQos& qos,
             SubscriberListener* listener = nullptr,
             const ::dds::core::status::StatusMask& mask = ::dds::core::status::StatusMask::none()) {
    auto impl = std::make_shared<rustorb::dds::detail::subscriber_impl>();
    impl->participant = dp.rustorb_impl();
    impl->qos = qos;
    auto names = rustorb::dds::detail::c_strings(qos.policy<::dds::core::policy::Partition>().name());
    rdds_group_qos c{names.data(), names.size()};
    rdds_subscriber* s = nullptr;
    rustorb::dds::detail::check(
        rdds_participant_create_subscriber(rustorb::dds::detail::deref(impl->participant).get(), &c, &s));
    impl->s.h = s;
    impl_ = impl;
    if (listener) this->listener(listener, mask);
  }

  // Sets the listener (nullptr clears it); the caller keeps it alive.
  void listener(SubscriberListener* l, const ::dds::core::status::StatusMask& mask) {
    rustorb::dds::detail::set_group_listener(impl().get(), l, mask.c(), impl().participant, impl_,
                                             rdds_subscriber_set_listener);
    impl().listener = l;
  }
  SubscriberListener* listener() const { return impl().listener; }

  // Calls on_data_available of the listeners of its readers that have new
  // data (from on_data_on_readers).
  void notify_datareaders() { rustorb::dds::detail::check(rdds_subscriber_notify_datareaders(impl().get())); }

  const qos::SubscriberQos& qos() const { return impl().qos; }
  const qos::DataReaderQos& default_datareader_qos() const { return impl().default_reader_qos; }
  Subscriber& default_datareader_qos(const qos::DataReaderQos& q) {
    impl().default_reader_qos = q;
    return *this;
  }
  ::dds::domain::DomainParticipant participant() const {
    return ::dds::domain::rustorb_participant(impl().participant);
  }

  void enable() {}
  void close() { impl().s.reset(); }
  void retain() {}

  friend bool operator==(const Subscriber& a, const Subscriber& b) { return a.impl_ == b.impl_; }
  friend bool operator!=(const Subscriber& a, const Subscriber& b) { return a.impl_ != b.impl_; }
  friend bool operator==(const Subscriber& a, const ::dds::core::null_type&) { return !a.impl_; }
  friend bool operator!=(const Subscriber& a, const ::dds::core::null_type&) { return !!a.impl_; }

  // rustorb
  const std::shared_ptr<rustorb::dds::detail::subscriber_impl>& rustorb_impl() const { return impl_; }
  static Subscriber rustorb_from(std::shared_ptr<rustorb::dds::detail::subscriber_impl> impl) {
    Subscriber s(::dds::core::null);
    s.impl_ = std::move(impl);
    return s;
  }

 private:
  rustorb::dds::detail::subscriber_impl& impl() const { return rustorb::dds::detail::deref(impl_); }
  std::shared_ptr<rustorb::dds::detail::subscriber_impl> impl_;
};

// SampleInfo (DDS-PSM-Cxx 1.0, 7.5.11).
class GenerationCount {
 public:
  GenerationCount(int32_t disposed = 0, int32_t no_writers = 0) : disposed_(disposed), no_writers_(no_writers) {}
  int32_t disposed() const { return disposed_; }
  int32_t no_writers() const { return no_writers_; }

 private:
  int32_t disposed_, no_writers_;
};

class Rank {
 public:
  Rank(int32_t sample = 0, int32_t generation = 0, int32_t absolute_generation = 0)
      : sample_(sample), generation_(generation), absolute_generation_(absolute_generation) {}
  int32_t sample() const { return sample_; }
  int32_t generation() const { return generation_; }
  int32_t absolute_generation() const { return absolute_generation_; }

 private:
  int32_t sample_, generation_, absolute_generation_;
};

class SampleInfo {
 public:
  SampleInfo() : i_{} {}
  explicit SampleInfo(const rdds_sample_info& i) : i_(i) {}
  ::dds::core::Time timestamp() const { return ::dds::core::Time::from_c(i_.source_timestamp); }
  status::DataState state() const {
    return status::DataState(status::SampleState(i_.sample_state), status::ViewState(i_.view_state),
                             status::InstanceState(i_.instance_state));
  }
  GenerationCount generation_count() const {
    return GenerationCount(i_.disposed_generation_count, i_.no_writers_generation_count);
  }
  Rank rank() const { return Rank(i_.sample_rank, i_.generation_rank, i_.absolute_generation_rank); }
  bool valid() const { return i_.valid_data; }
  ::dds::core::InstanceHandle instance_handle() const { return ::dds::core::InstanceHandle(i_.instance_handle); }
  ::dds::core::InstanceHandle publication_handle() const {
    return ::dds::core::InstanceHandle(i_.publication_handle);
  }
  // rustorb: the writer's sequence number of the sample.
  int64_t sequence_number() const { return i_.sequence_number; }

 private:
  rdds_sample_info i_;
};

template <typename T>
class Sample {
 public:
  Sample() = default;
  Sample(const T& data, const SampleInfo& info) : data_(data), info_(info) {}
  const T& data() const { return data_; }
  T& data() { return data_; }
  void data(const T& d) { data_ = d; }
  const SampleInfo& info() const { return info_; }
  void info(const SampleInfo& i) { info_ = i; }

 private:
  T data_{};
  SampleInfo info_;
};

// The samples a read or take gave; they are copies, so returning the loan
// does nothing.
template <typename T>
class LoanedSamples {
 public:
  using value_type = Sample<T>;
  using const_iterator = typename std::vector<Sample<T>>::const_iterator;
  using iterator = const_iterator;

  LoanedSamples() = default;
  explicit LoanedSamples(std::vector<Sample<T>> s) : s_(std::make_shared<std::vector<Sample<T>>>(std::move(s))) {}
  const_iterator begin() const { return s_->begin(); }
  const_iterator end() const { return s_->end(); }
  uint32_t length() const { return static_cast<uint32_t>(s_->size()); }
  const Sample<T>& operator[](size_t i) const { return (*s_)[i]; }
  void return_loan() {}

 private:
  std::shared_ptr<std::vector<Sample<T>>> s_ = std::make_shared<std::vector<Sample<T>>>();
};

template <typename T>
class DataReaderListener {
 public:
  virtual ~DataReaderListener() = default;
  virtual void on_requested_deadline_missed(DataReader<T>& reader,
                                            const ::dds::core::status::RequestedDeadlineMissedStatus& status) = 0;
  virtual void on_requested_incompatible_qos(DataReader<T>& reader,
                                             const ::dds::core::status::RequestedIncompatibleQosStatus& status) = 0;
  virtual void on_sample_rejected(DataReader<T>& reader, const ::dds::core::status::SampleRejectedStatus& status) = 0;
  virtual void on_liveliness_changed(DataReader<T>& reader,
                                     const ::dds::core::status::LivelinessChangedStatus& status) = 0;
  virtual void on_data_available(DataReader<T>& reader) = 0;
  virtual void on_subscription_matched(DataReader<T>& reader,
                                       const ::dds::core::status::SubscriptionMatchedStatus& status) = 0;
  virtual void on_sample_lost(DataReader<T>& reader, const ::dds::core::status::SampleLostStatus& status) = 0;
};

template <typename T>
class NoOpDataReaderListener : public virtual DataReaderListener<T> {
 public:
  void on_requested_deadline_missed(DataReader<T>&,
                                    const ::dds::core::status::RequestedDeadlineMissedStatus&) override {}
  void on_requested_incompatible_qos(DataReader<T>&,
                                     const ::dds::core::status::RequestedIncompatibleQosStatus&) override {}
  void on_sample_rejected(DataReader<T>&, const ::dds::core::status::SampleRejectedStatus&) override {}
  void on_liveliness_changed(DataReader<T>&, const ::dds::core::status::LivelinessChangedStatus&) override {}
  void on_data_available(DataReader<T>&) override {}
  void on_subscription_matched(DataReader<T>&, const ::dds::core::status::SubscriptionMatchedStatus&) override {}
  void on_sample_lost(DataReader<T>&, const ::dds::core::status::SampleLostStatus&) override {}
};

namespace cond {
class ReadCondition;
}

template <typename T>
class DataReader {
  using impl_type = rustorb::dds::detail::reader_impl<T>;
  using box = rustorb::dds::detail::listener_box<impl_type, DataReaderListener<T>>;

 public:
  using DataType = T;

  // What to read or take: dr.select().instance(h).state(s).take().
  class Selector {
   public:
    explicit Selector(DataReader& dr) : dr_(dr), state_(dr.default_filter_state()) {}
    Selector& instance(const ::dds::core::InstanceHandle& h) {
      mode_ = RDDS_SELECT_INSTANCE;
      handle_ = h;
      return *this;
    }
    Selector& next_instance(const ::dds::core::InstanceHandle& h) {
      mode_ = RDDS_SELECT_NEXT_INSTANCE;
      handle_ = h;
      return *this;
    }
    Selector& state(const status::DataState& s) {
      state_ = s;
      return *this;
    }
    Selector& max_samples(uint32_t n) {
      max_ = n;
      return *this;
    }
    // rustorb: the samples of a ReadCondition of this reader.
    Selector& condition(const cond::ReadCondition& c);

    LoanedSamples<T> read() { return dr_.select_c(false, *this); }
    LoanedSamples<T> take() { return dr_.select_c(true, *this); }

   private:
    friend class DataReader;
    DataReader& dr_;
    int32_t mode_ = RDDS_SELECT_ANY;
    ::dds::core::InstanceHandle handle_;
    status::DataState state_;
    uint32_t max_ = 0;
    const rdds_condition* condition_ = nullptr;
    std::shared_ptr<void> condition_owner_;
  };

  DataReader(const ::dds::core::null_type&) {}
  DataReader(const Subscriber& sub, const ::dds::topic::Topic<T>& topic)
      : DataReader(sub, topic, sub.default_datareader_qos()) {}
  DataReader(const Subscriber& sub, const ::dds::topic::Topic<T>& topic, const qos::DataReaderQos& qos,
             DataReaderListener<T>* listener = nullptr,
             const ::dds::core::status::StatusMask& mask = ::dds::core::status::StatusMask::all()) {
    auto impl = std::make_shared<impl_type>();
    impl->subscriber = sub.rustorb_impl();
    impl->topic = topic.rustorb_impl();
    impl->qos = qos;
    impl->listener = listener;
    impl->mask = mask;
    rdds_reader_qos c;
    rustorb::dds::detail::to_c(qos, c);
    box* b = listener ? new box(listener) : nullptr;
    rdds_reader_listener l = table(b);
    rdds_reader* r = nullptr;
    rustorb::dds::detail::check(rdds_subscriber_create_reader(rustorb::dds::detail::deref(impl->subscriber).get(),
                                                              rustorb::dds::detail::deref(impl->topic).get(), &c,
                                                              b ? &l : nullptr, mask.c(), &r));
    impl->r.h = r;
    impl_ = impl;
    if (b) b->ready(impl_);
  }
  DataReader(const Subscriber& sub, const ::dds::topic::ContentFilteredTopic<T>& topic)
      : DataReader(sub, topic, sub.default_datareader_qos()) {}
  DataReader(const Subscriber& sub, const ::dds::topic::ContentFilteredTopic<T>& topic,
             const qos::DataReaderQos& qos, DataReaderListener<T>* listener = nullptr,
             const ::dds::core::status::StatusMask& mask = ::dds::core::status::StatusMask::all()) {
    auto impl = std::make_shared<impl_type>();
    impl->subscriber = sub.rustorb_impl();
    impl->topic = topic.topic().rustorb_impl();
    impl->cftopic = topic.rustorb_impl();
    impl->qos = qos;
    impl->listener = listener;
    impl->mask = mask;
    rdds_reader_qos c;
    rustorb::dds::detail::to_c(qos, c);
    box* b = listener ? new box(listener) : nullptr;
    rdds_reader_listener l = table(b);
    rdds_reader* r = nullptr;
    rustorb::dds::detail::check(rdds_subscriber_create_filtered_reader(
        rustorb::dds::detail::deref(impl->subscriber).get(), rustorb::dds::detail::deref(impl->cftopic).get(), &c,
        b ? &l : nullptr, mask.c(), &r));
    impl->r.h = r;
    impl_ = impl;
    if (b) b->ready(impl_);
  }

  const status::DataState& default_filter_state() { return impl().default_filter_state; }
  DataReader& default_filter_state(const status::DataState& s) {
    impl().default_filter_state = s;
    return *this;
  }

  LoanedSamples<T> read() { return select().read(); }
  LoanedSamples<T> take() { return select().take(); }
  DataReader& operator>>(LoanedSamples<T>& ls) {
    ls = take();
    return *this;
  }
  Selector select() { return Selector(*this); }

  T& key_value(T& sample, const ::dds::core::InstanceHandle& h) {
    rdds_in* in = nullptr;
    rustorb::dds::detail::check(rdds_reader_get_key_value(impl().get(), h.c(), &in));
    rustorb::dds::in_stream s(in);
    rustorb::dds::read_sample(s.get(), sample);
    return sample;
  }
  const ::dds::core::InstanceHandle lookup_instance(const T& key) const {
    rustorb::dds::out_stream s;
    rustorb::dds::write_sample(s, key);
    rdds_instance_handle h;
    rustorb::dds::detail::check(rdds_reader_lookup_instance(impl().get(), s.get(), &h));
    return ::dds::core::InstanceHandle(h);
  }

  const qos::DataReaderQos& qos() const { return impl().qos; }
  void qos(const qos::DataReaderQos&) {
    throw ::dds::core::UnsupportedError("the QoS of a DataReader cannot be changed yet");
  }
  const ::dds::topic::Topic<T> topic() const { return ::dds::topic::Topic<T>::rustorb_from(impl().topic); }
  const ::dds::topic::Topic<T> topic_description() const { return topic(); }
  const Subscriber subscriber() const { return Subscriber::rustorb_from(impl().subscriber); }

  // rustorb: until a sample is there; false on timeout.
  bool wait_for_data(const ::dds::core::Duration& timeout) {
    rdds_status st = rdds_reader_wait_for_data(impl().get(), timeout.c());
    if (st == RDDS_RETCODE_TIMEOUT) return false;
    rustorb::dds::detail::check(st);
    return true;
  }
  // rustorb: until count writers are matched; false on timeout.
  bool wait_for_publications(size_t count, const ::dds::core::Duration& timeout) {
    rdds_status st = rdds_reader_wait_for_publications(impl().get(), count, timeout.c());
    if (st == RDDS_RETCODE_TIMEOUT) return false;
    rustorb::dds::detail::check(st);
    return true;
  }
  ::dds::core::InstanceHandleSeq matched_publications() const {
    std::vector<rdds_guid> buf(16);
    size_t len = 0;
    for (;;) {
      rustorb::dds::detail::check(rdds_reader_matched_publications(impl().get(), buf.data(), buf.size(), &len));
      if (len <= buf.size()) break;
      buf.resize(len);
    }
    ::dds::core::InstanceHandleSeq out;
    for (size_t i = 0; i < len; i++) out.emplace_back(buf[i]);
    return out;
  }

  void listener(DataReaderListener<T>* l, const ::dds::core::status::StatusMask& mask) {
    box* b = l ? new box(l) : nullptr;
    if (b) b->ready(impl_);
    rdds_reader_listener t = table(b);
    rustorb::dds::detail::check(rdds_reader_set_listener(impl().get(), b ? &t : nullptr, mask.c()));
    impl().listener = l;
    impl().mask = mask;
  }
  DataReaderListener<T>* listener() const { return impl().listener; }

  const ::dds::core::status::LivelinessChangedStatus liveliness_changed_status() {
    rdds_liveliness_changed_status s;
    rustorb::dds::detail::check(rdds_reader_liveliness_changed_status(impl().get(), &s));
    return ::dds::core::status::LivelinessChangedStatus(s);
  }
  const ::dds::core::status::SampleRejectedStatus sample_rejected_status() {
    rdds_sample_rejected_status s;
    rustorb::dds::detail::check(rdds_reader_sample_rejected_status(impl().get(), &s));
    return ::dds::core::status::SampleRejectedStatus(s);
  }
  const ::dds::core::status::SampleLostStatus sample_lost_status() {
    rdds_count_status s;
    rustorb::dds::detail::check(rdds_reader_sample_lost_status(impl().get(), &s));
    return ::dds::core::status::SampleLostStatus(s);
  }
  const ::dds::core::status::RequestedDeadlineMissedStatus requested_deadline_missed_status() {
    rdds_deadline_missed_status s;
    rustorb::dds::detail::check(rdds_reader_requested_deadline_missed_status(impl().get(), &s));
    return ::dds::core::status::RequestedDeadlineMissedStatus(s);
  }
  const ::dds::core::status::RequestedIncompatibleQosStatus requested_incompatible_qos_status() {
    rdds_incompatible_qos_status s;
    rustorb::dds::detail::check(rdds_reader_requested_incompatible_qos_status(impl().get(), &s));
    return ::dds::core::status::RequestedIncompatibleQosStatus(s);
  }
  const ::dds::core::status::SubscriptionMatchedStatus subscription_matched_status() {
    rdds_matched_status s;
    rustorb::dds::detail::check(rdds_reader_subscription_matched_status(impl().get(), &s));
    return ::dds::core::status::SubscriptionMatchedStatus(s);
  }

  ::dds::core::status::StatusMask status_changes() const {
    return ::dds::core::status::StatusMask(rdds_reader_status_changes(impl().get()));
  }
  ::dds::core::InstanceHandle instance_handle() const {
    rdds_guid g;
    rustorb::dds::detail::check(rdds_reader_guid(impl().get(), &g));
    return ::dds::core::InstanceHandle(g);
  }

  void enable() {}
  void close() { impl().r.reset(); }
  void retain() {}

  friend bool operator==(const DataReader& a, const DataReader& b) { return a.impl_ == b.impl_; }
  friend bool operator!=(const DataReader& a, const DataReader& b) { return a.impl_ != b.impl_; }
  friend bool operator==(const DataReader& a, const ::dds::core::null_type&) { return !a.impl_; }
  friend bool operator!=(const DataReader& a, const ::dds::core::null_type&) { return !!a.impl_; }

  // rustorb
  rdds_condition* rustorb_status_condition() const {
    rdds_condition* c = nullptr;
    rustorb::dds::detail::check(rdds_reader_status_condition(impl().get(), &c));
    return c;
  }
  rdds_reader* rustorb_handle() const { return impl().get(); }
  std::shared_ptr<void> rustorb_owner() const { return impl_; }

 private:
  impl_type& impl() const { return rustorb::dds::detail::deref(impl_); }

  explicit DataReader(std::shared_ptr<impl_type> impl) : impl_(std::move(impl)) {}

  LoanedSamples<T> select_c(bool take, const Selector& sel) {
    if (take && sel.max_ != 0)
      throw ::dds::core::UnsupportedError("max_samples with take is not supported yet");
    rdds_samples* samples = nullptr;
    if (sel.condition_) {
      rustorb::dds::detail::check(rdds_reader_select_w_condition(impl().get(), take, sel.condition_, &samples));
    } else {
      rdds_instance_handle h = sel.handle_.c();
      rustorb::dds::detail::check(rdds_reader_select(impl().get(), take, sel.state_.sample_state().c(),
                                                     sel.state_.view_state().c(), sel.state_.instance_state().c(),
                                                     sel.mode_, &h, &samples));
    }
    rustorb::dds::detail::handle<rdds_samples, rdds_samples_release> owner(samples);
    size_t n = rdds_samples_count(samples);
    if (sel.max_ != 0 && n > sel.max_) n = sel.max_;
    std::vector<Sample<T>> out(n);
    for (size_t i = 0; i < n; i++) {
      rdds_sample_info info;
      rustorb::dds::detail::check(rdds_samples_info(samples, i, &info));
      out[i].info(SampleInfo(info));
      rdds_in* in = nullptr;
      if (info.valid_data) {
        rustorb::dds::detail::check(rdds_samples_data(samples, i, &in));
      } else if (rdds_reader_get_key_value(impl().get(), info.instance_handle, &in) != RDDS_RETCODE_OK) {
        in = nullptr;  // the key is not known: a default sample
      }
      if (in) {
        rustorb::dds::in_stream s(in);
        rustorb::dds::read_sample(s.get(), out[i].data());
      }
    }
    return LoanedSamples<T>(std::move(out));
  }

  static rdds_reader_listener table(box* b) {
    rdds_reader_listener l{};
    if (!b) return l;
    l.self = b;
    using L = DataReaderListener<T>;
    using ip = std::shared_ptr<impl_type>;
    l.on_requested_deadline_missed = [](void* self, const rdds_deadline_missed_status* s) {
      ::dds::core::status::RequestedDeadlineMissedStatus st(*s);
      static_cast<box*>(self)->call([st](const ip& i, L* l) {
        DataReader r(i);
        l->on_requested_deadline_missed(r, st);
      });
    };
    l.on_requested_incompatible_qos = [](void* self, const rdds_incompatible_qos_status* s) {
      ::dds::core::status::RequestedIncompatibleQosStatus st(*s);
      static_cast<box*>(self)->call([st](const ip& i, L* l) {
        DataReader r(i);
        l->on_requested_incompatible_qos(r, st);
      });
    };
    l.on_sample_rejected = [](void* self, const rdds_sample_rejected_status* s) {
      ::dds::core::status::SampleRejectedStatus st(*s);
      static_cast<box*>(self)->call([st](const ip& i, L* l) {
        DataReader r(i);
        l->on_sample_rejected(r, st);
      });
    };
    l.on_liveliness_changed = [](void* self, const rdds_liveliness_changed_status* s) {
      ::dds::core::status::LivelinessChangedStatus st(*s);
      static_cast<box*>(self)->call([st](const ip& i, L* l) {
        DataReader r(i);
        l->on_liveliness_changed(r, st);
      });
    };
    l.on_data_available = [](void* self) {
      static_cast<box*>(self)->call([](const ip& i, L* l) {
        DataReader r(i);
        l->on_data_available(r);
      });
    };
    l.on_subscription_matched = [](void* self, const rdds_matched_status* s) {
      ::dds::core::status::SubscriptionMatchedStatus st(*s);
      static_cast<box*>(self)->call([st](const ip& i, L* l) {
        DataReader r(i);
        l->on_subscription_matched(r, st);
      });
    };
    l.on_sample_lost = [](void* self, const rdds_count_status* s) {
      ::dds::core::status::SampleLostStatus st(*s);
      static_cast<box*>(self)->call([st](const ip& i, L* l) {
        DataReader r(i);
        l->on_sample_lost(r, st);
      });
    };
    l.release = &box::release;
    return l;
  }

  std::shared_ptr<impl_type> impl_;
};

}  // namespace sub

// ---------------------------------------------------------------------------
// Conditions and WaitSets (DDS-PSM-Cxx 1.0, 7.5.12)

namespace core {
namespace cond {

class Condition {
 public:
  Condition(const ::dds::core::null_type&) {}

  bool trigger_value() const { return rdds_condition_trigger_value(impl().c.h); }

  // A function dispatch() calls when the condition is true.
  template <typename F>
  void handler(F&& f) {
    impl().handler = std::function<void()>(std::forward<F>(f));
  }
  void reset_handler() { impl().handler = nullptr; }
  void dispatch() const {
    if (impl().handler) impl().handler();
  }

  friend bool operator==(const Condition& a, const Condition& b) {
    if (!a.impl_ || !b.impl_) return a.impl_ == b.impl_;
    return a.impl_ == b.impl_ || rdds_condition_equals(a.impl_->c.h, b.impl_->c.h);
  }
  friend bool operator!=(const Condition& a, const Condition& b) { return !(a == b); }
  friend bool operator==(const Condition& a, const ::dds::core::null_type&) { return !a.impl_; }
  friend bool operator!=(const Condition& a, const ::dds::core::null_type&) { return !!a.impl_; }

  // rustorb
  const rdds_condition* rustorb_handle() const { return impl().c.h; }
  const std::shared_ptr<void>& rustorb_owner() const { return impl().owner; }

 protected:
  Condition() = default;
  explicit Condition(rdds_condition* c, std::shared_ptr<void> owner = nullptr)
      : impl_(std::make_shared<rustorb::dds::detail::condition_impl>()) {
    impl_->c.h = c;
    impl_->owner = std::move(owner);
  }
  rustorb::dds::detail::condition_impl& impl() const { return rustorb::dds::detail::deref(impl_); }

 private:
  std::shared_ptr<rustorb::dds::detail::condition_impl> impl_;
};

class GuardCondition : public Condition {
 public:
  GuardCondition() : Condition(make()) {}
  GuardCondition(const ::dds::core::null_type& n) : Condition(n) {}
  template <typename F>
  explicit GuardCondition(F&& f) : GuardCondition() {
    handler(std::forward<F>(f));
  }
  void trigger_value(bool b) {
    rustorb::dds::detail::check(rdds_guard_condition_set_trigger_value(impl().c.h, b));
  }
  bool trigger_value() const { return Condition::trigger_value(); }

 private:
  static rdds_condition* make() {
    rdds_condition* c = nullptr;
    rustorb::dds::detail::check(rdds_guard_condition_new(&c));
    return c;
  }
};

class StatusCondition : public Condition {
 public:
  StatusCondition(const ::dds::core::null_type& n) : Condition(n) {}
  // The StatusCondition of a DataWriter or DataReader.
  template <typename Entity>
  explicit StatusCondition(const Entity& e) : Condition(e.rustorb_status_condition(), e.rustorb_owner()) {}
  template <typename Entity, typename F>
  StatusCondition(const Entity& e, F&& f) : StatusCondition(e) {
    handler(std::forward<F>(f));
  }

  void enabled_statuses(const ::dds::core::status::StatusMask& mask) const {
    rustorb::dds::detail::check(rdds_status_condition_set_enabled_statuses(impl().c.h, mask.c()));
  }
  const ::dds::core::status::StatusMask enabled_statuses() const {
    uint32_t m = 0;
    rustorb::dds::detail::check(rdds_status_condition_get_enabled_statuses(impl().c.h, &m));
    return ::dds::core::status::StatusMask(m);
  }
};

using ConditionSeq = std::vector<Condition>;

class WaitSet {
 public:
  WaitSet() : impl_(std::make_shared<state>()) {
    rdds_waitset* w = nullptr;
    rustorb::dds::detail::check(rdds_waitset_new(&w));
    impl_->w.h = w;
  }
  WaitSet(const ::dds::core::null_type&) {}

  // The conditions that are true; TimeoutError if none is within timeout.
  const ConditionSeq wait(const ::dds::core::Duration& timeout = ::dds::core::Duration::infinite()) {
    state& s = rustorb::dds::detail::deref(impl_);
    std::vector<size_t> active(s.attached.size() + 1);
    size_t len = 0;
    rustorb::dds::detail::check(rdds_waitset_wait(s.w.h, timeout.c(), active.data(), active.size(), &len));
    ConditionSeq out;
    for (size_t i = 0; i < len && i < active.size(); i++)
      if (active[i] < s.attached.size()) out.push_back(s.attached[active[i]]);
    return out;
  }
  ConditionSeq& wait(ConditionSeq& triggered, const ::dds::core::Duration& timeout = ::dds::core::Duration::infinite()) {
    triggered = wait(timeout);
    return triggered;
  }
  // Waits, then calls the handler of each condition that is true.
  void dispatch(const ::dds::core::Duration& timeout = ::dds::core::Duration::infinite()) {
    for (auto& c : wait(timeout)) c.dispatch();
  }

  WaitSet& attach_condition(const Condition& c) {
    state& s = rustorb::dds::detail::deref(impl_);
    rustorb::dds::detail::check(rdds_waitset_attach_condition(s.w.h, c.rustorb_handle()));
    s.attached.push_back(c);
    return *this;
  }
  bool detach_condition(const Condition& c) {
    state& s = rustorb::dds::detail::deref(impl_);
    for (size_t i = 0; i < s.attached.size(); i++) {
      if (s.attached[i] == c) {
        rustorb::dds::detail::check(rdds_waitset_detach_condition(s.w.h, c.rustorb_handle()));
        s.attached.erase(s.attached.begin() + static_cast<std::ptrdiff_t>(i));
        return true;
      }
    }
    return false;
  }
  WaitSet& operator+=(const Condition& c) { return attach_condition(c); }
  WaitSet& operator-=(const Condition& c) {
    detach_condition(c);
    return *this;
  }
  const ConditionSeq conditions() const { return rustorb::dds::detail::deref(impl_).attached; }

 private:
  struct state {
    rustorb::dds::detail::handle<rdds_waitset, rdds_waitset_release> w;
    ConditionSeq attached;
  };
  std::shared_ptr<state> impl_;
};

}  // namespace cond
}  // namespace core

namespace sub {
namespace cond {

// The samples of a DataReader in some states, as a condition.
class ReadCondition : public ::dds::core::cond::Condition {
 public:
  ReadCondition(const ::dds::core::null_type& n) : Condition(n) {}
  template <typename T>
  ReadCondition(const DataReader<T>& dr, const status::DataState& state)
      : Condition(make(dr.rustorb_handle(), state), dr.rustorb_owner()), state_(state) {}
  template <typename T, typename F>
  ReadCondition(const DataReader<T>& dr, const status::DataState& state, F&& f) : ReadCondition(dr, state) {
    handler(std::forward<F>(f));
  }
  const status::DataState& state_filter() const { return state_; }

 private:
  static rdds_condition* make(rdds_reader* r, const status::DataState& s) {
    rdds_condition* c = nullptr;
    rustorb::dds::detail::check(rdds_reader_create_readcondition(r, s.sample_state().c(), s.view_state().c(),
                                                                 s.instance_state().c(), &c));
    return c;
  }
  status::DataState state_;
};

}  // namespace cond

template <typename T>
typename DataReader<T>::Selector& DataReader<T>::Selector::condition(const cond::ReadCondition& c) {
  condition_ = c.rustorb_handle();
  condition_owner_ = c.rustorb_owner();
  return *this;
}

}  // namespace sub

namespace domain {
inline DomainParticipant rustorb_participant(std::shared_ptr<rustorb::dds::detail::participant_impl> impl) {
  return DomainParticipant::rustorb_from(std::move(impl));
}
}  // namespace domain

namespace topic {
template <typename T>
::dds::domain::DomainParticipant Topic<T>::domain_participant() const {
  return ::dds::domain::rustorb_participant(impl().participant);
}
}  // namespace topic

}  // namespace dds

namespace rustorb {
namespace dds {
namespace detail {

template <typename L>
rdds_listener group_table(group_box<L>* b) {
  using B = group_box<L>;
  namespace st = ::dds::core::status;
  rdds_listener l{};
  l.self = b;
  l.release = &B::release;
  if constexpr (std::is_base_of<::dds::pub::AnyDataWriterListener, L>::value) {
    l.on_offered_deadline_missed = [](void* self, const rdds_endpoint_info* w, const rdds_deadline_missed_status* s) {
      ::dds::pub::AnyDataWriter a(*w);
      static_cast<B*>(self)->listener->on_offered_deadline_missed(a, st::OfferedDeadlineMissedStatus(*s));
    };
    l.on_offered_incompatible_qos = [](void* self, const rdds_endpoint_info* w,
                                       const rdds_incompatible_qos_status* s) {
      ::dds::pub::AnyDataWriter a(*w);
      static_cast<B*>(self)->listener->on_offered_incompatible_qos(a, st::OfferedIncompatibleQosStatus(*s));
    };
    l.on_liveliness_lost = [](void* self, const rdds_endpoint_info* w, const rdds_count_status* s) {
      ::dds::pub::AnyDataWriter a(*w);
      static_cast<B*>(self)->listener->on_liveliness_lost(a, st::LivelinessLostStatus(*s));
    };
    l.on_publication_matched = [](void* self, const rdds_endpoint_info* w, const rdds_matched_status* s) {
      ::dds::pub::AnyDataWriter a(*w);
      static_cast<B*>(self)->listener->on_publication_matched(a, st::PublicationMatchedStatus(*s));
    };
  }
  if constexpr (std::is_base_of<::dds::sub::AnyDataReaderListener, L>::value) {
    l.on_requested_deadline_missed = [](void* self, const rdds_endpoint_info* r,
                                        const rdds_deadline_missed_status* s) {
      ::dds::sub::AnyDataReader a(*r);
      static_cast<B*>(self)->listener->on_requested_deadline_missed(a, st::RequestedDeadlineMissedStatus(*s));
    };
    l.on_requested_incompatible_qos = [](void* self, const rdds_endpoint_info* r,
                                         const rdds_incompatible_qos_status* s) {
      ::dds::sub::AnyDataReader a(*r);
      static_cast<B*>(self)->listener->on_requested_incompatible_qos(a, st::RequestedIncompatibleQosStatus(*s));
    };
    l.on_sample_rejected = [](void* self, const rdds_endpoint_info* r, const rdds_sample_rejected_status* s) {
      ::dds::sub::AnyDataReader a(*r);
      static_cast<B*>(self)->listener->on_sample_rejected(a, st::SampleRejectedStatus(*s));
    };
    l.on_liveliness_changed = [](void* self, const rdds_endpoint_info* r, const rdds_liveliness_changed_status* s) {
      ::dds::sub::AnyDataReader a(*r);
      static_cast<B*>(self)->listener->on_liveliness_changed(a, st::LivelinessChangedStatus(*s));
    };
    l.on_data_available = [](void* self, const rdds_endpoint_info* r) {
      ::dds::sub::AnyDataReader a(*r);
      static_cast<B*>(self)->listener->on_data_available(a);
    };
    l.on_subscription_matched = [](void* self, const rdds_endpoint_info* r, const rdds_matched_status* s) {
      ::dds::sub::AnyDataReader a(*r);
      static_cast<B*>(self)->listener->on_subscription_matched(a, st::SubscriptionMatchedStatus(*s));
    };
    l.on_sample_lost = [](void* self, const rdds_endpoint_info* r, const rdds_count_status* s) {
      ::dds::sub::AnyDataReader a(*r);
      static_cast<B*>(self)->listener->on_sample_lost(a, st::SampleLostStatus(*s));
    };
  }
  if constexpr (std::is_base_of<::dds::sub::SubscriberListener, L>::value) {
    l.on_data_on_readers = [](void* self, const rdds_subscriber* s) {
      auto* b = static_cast<B*>(self);
      std::shared_ptr<subscriber_impl> impl = b->subscriber.lock();
      if (!impl || !rdds_subscriber_same(impl->s.h, s)) {
        auto participant = b->participant.lock();
        if (!participant) return;
        rdds_subscriber* kept = nullptr;
        if (rdds_subscriber_clone(s, &kept) != RDDS_RETCODE_OK) return;
        impl = std::make_shared<subscriber_impl>();
        impl->participant = participant;
        impl->s.h = kept;
      }
      ::dds::sub::Subscriber sub = ::dds::sub::Subscriber::rustorb_from(impl);
      b->listener->on_data_on_readers(sub);
    };
  }
  if constexpr (std::is_base_of<::dds::topic::AnyTopicListener, L>::value) {
    l.on_inconsistent_topic = [](void* self, const char* name, const rdds_count_status* s) {
      ::dds::topic::AnyTopic t(name);
      static_cast<B*>(self)->listener->on_inconsistent_topic(t, st::InconsistentTopicStatus(*s));
    };
  }
  return l;
}

}  // namespace detail

inline ::dds::domain::DomainParticipant localhost_participant(uint32_t domain_id) {
  return ::dds::domain::DomainParticipant(domain_id, ::dds::domain::qos::DomainParticipantQos(), nullptr,
                                          ::dds::core::status::StatusMask::none(), RDDS_PARTICIPANT_LOCALHOST);
}
}  // namespace dds
}  // namespace rustorb

#endif  // RUSTORB_DDS_ENTITIES_HPP

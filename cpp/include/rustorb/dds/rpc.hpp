// rustorb/dds/rpc.hpp: requests and replies over DDS (DDS-RPC 1.0, the
// request/reply style): dds::rpc::Requester and dds::rpc::Replier, over
// the requesters and repliers of the C ABI.
//
// A Requester sends requests on a request topic and gets the replies to
// them on a reply topic; a Replier takes the requests and answers each.
// A reply names its request by its SampleIdentity, sent in-line; see
// docs/rpc.md.

#ifndef RUSTORB_DDS_RPC_HPP
#define RUSTORB_DDS_RPC_HPP

#include <cstring>
#include <memory>
#include <string>

#include <rustorb/dds/entities.hpp>

namespace dds {
namespace rpc {

// The identity of a sample: its writer and sequence number. A request's
// identity is what its replies name.
class SampleIdentity {
 public:
  SampleIdentity() : c_{} {}
  explicit SampleIdentity(const rdds_sample_identity& c) : c_(c) {}
  int64_t sequence_number() const { return c_.sequence_number; }
  ::dds::core::InstanceHandle writer_guid() const {
    rdds_instance_handle h;
    std::memcpy(h.value, c_.writer.value, sizeof h.value);
    return ::dds::core::InstanceHandle(h);
  }
  const rdds_sample_identity& c() const { return c_; }
  friend bool operator==(const SampleIdentity& a, const SampleIdentity& b) {
    return a.c_.sequence_number == b.c_.sequence_number &&
           std::memcmp(a.c_.writer.value, b.c_.writer.value, sizeof a.c_.writer.value) == 0;
  }
  friend bool operator!=(const SampleIdentity& a, const SampleIdentity& b) { return !(a == b); }

 private:
  rdds_sample_identity c_;
};

// How a Requester or Replier is made: its service name (the topics
// <service>_Request and <service>_Reply, DDS-RPC 1.0, 7.4.1) or the topic
// names, and its QoS (RELIABLE and KEEP_ALL if not given).
class ServiceParams {
 public:
  ServiceParams() = default;
  explicit ServiceParams(const ::dds::domain::DomainParticipant& dp) : participant_(dp) {}

  ServiceParams& domain_participant(const ::dds::domain::DomainParticipant& dp) {
    participant_ = dp;
    return *this;
  }
  ServiceParams& service_name(const std::string& name) {
    service_name_ = name;
    return *this;
  }
  ServiceParams& request_topic_name(const std::string& name) {
    request_topic_ = name;
    return *this;
  }
  ServiceParams& reply_topic_name(const std::string& name) {
    reply_topic_ = name;
    return *this;
  }
  ServiceParams& instance_name(const std::string& name) {
    instance_name_ = name;
    return *this;
  }
  ServiceParams& datawriter_qos(const ::dds::pub::qos::DataWriterQos& q) {
    writer_qos_ = std::make_shared<::dds::pub::qos::DataWriterQos>(q);
    return *this;
  }
  ServiceParams& datareader_qos(const ::dds::sub::qos::DataReaderQos& q) {
    reader_qos_ = std::make_shared<::dds::sub::qos::DataReaderQos>(q);
    return *this;
  }

  const ::dds::domain::DomainParticipant& domain_participant() const { return participant_; }
  std::string request_topic_name() const {
    return request_topic_.empty() ? service_name_ + "_Request" : request_topic_;
  }
  std::string reply_topic_name() const { return reply_topic_.empty() ? service_name_ + "_Reply" : reply_topic_; }
  const std::string& instance_name() const { return instance_name_; }
  const ::dds::pub::qos::DataWriterQos* datawriter_qos() const { return writer_qos_.get(); }
  const ::dds::sub::qos::DataReaderQos* datareader_qos() const { return reader_qos_.get(); }

 private:
  ::dds::domain::DomainParticipant participant_ = ::dds::core::null;
  std::string service_name_;
  std::string request_topic_;
  std::string reply_topic_;
  std::string instance_name_;
  std::shared_ptr<::dds::pub::qos::DataWriterQos> writer_qos_;
  std::shared_ptr<::dds::sub::qos::DataReaderQos> reader_qos_;
};

using RequesterParams = ServiceParams;
using ReplierParams = ServiceParams;

namespace detail {
struct qos_c {
  rdds_writer_qos w;
  rdds_reader_qos r;
  const rdds_writer_qos* wp = nullptr;
  const rdds_reader_qos* rp = nullptr;
  explicit qos_c(const ServiceParams& p) {
    if (p.datawriter_qos()) {
      rustorb::dds::detail::to_c(*p.datawriter_qos(), w);
      wp = &w;
    }
    if (p.datareader_qos()) {
      rustorb::dds::detail::to_c(*p.datareader_qos(), r);
      rp = &r;
    }
  }
};
}  // namespace detail

// The client side of a service: sends requests of type TReq and gets the
// replies of type TRep to them.
template <typename TReq, typename TRep>
class Requester {
 public:
  explicit Requester(const RequesterParams& params)
      : participant_(params.domain_participant()),
        request_(participant_, params.request_topic_name()),
        reply_(participant_, params.reply_topic_name()) {
    detail::qos_c q(params);
    rdds_requester* r = nullptr;
    rustorb::dds::detail::check(rdds_requester_new(participant_.rustorb_impl()->get(),
                                                   request_.rustorb_impl()->get(), reply_.rustorb_impl()->get(),
                                                   q.wp, q.rp, &r));
    r_ = std::shared_ptr<rdds_requester>(r, rdds_requester_release);
  }

  // Waits until a replier is matched; false after max_wait.
  bool wait_for_service(const ::dds::core::Duration& max_wait) const {
    rdds_status st = rdds_requester_wait_for_service(r_.get(), max_wait.c());
    if (st == RDDS_RETCODE_TIMEOUT) return false;
    rustorb::dds::detail::check(st);
    return true;
  }

  // Sends a request; returns its identity.
  SampleIdentity send_request(const TReq& request) {
    rustorb::dds::out_stream s;
    rustorb::dds::write_sample(s, request);
    rdds_sample_identity id;
    rustorb::dds::detail::check(rdds_requester_send_request(r_.get(), s.get(), &id));
    return SampleIdentity(id);
  }

  // Waits for a reply to the request `id`; false after max_wait.
  bool receive_reply(TRep& reply, const SampleIdentity& id, const ::dds::core::Duration& max_wait) {
    rdds_in* in = nullptr;
    rdds_status st = rdds_requester_receive_reply(r_.get(), &id.c(), max_wait.c(), &in);
    if (st == RDDS_RETCODE_TIMEOUT) return false;
    rustorb::dds::detail::check(st);
    rustorb::dds::in_stream s(in);
    rustorb::dds::read_sample(s.get(), reply);
    return true;
  }

  // Sends a request and waits for its reply; throws TimeoutError after
  // max_wait.
  TRep request(const TReq& request, const ::dds::core::Duration& max_wait) {
    TRep reply;
    if (!receive_reply(reply, send_request(request), max_wait)) {
      throw ::dds::core::TimeoutError("no reply in time");
    }
    return reply;
  }

  ::dds::domain::DomainParticipant domain_participant() const { return participant_; }

 private:
  ::dds::domain::DomainParticipant participant_;
  ::dds::topic::Topic<TReq> request_;
  ::dds::topic::Topic<TRep> reply_;
  std::shared_ptr<rdds_requester> r_;
};

// The service side: takes requests of type TReq and answers each with
// replies of type TRep.
template <typename TReq, typename TRep>
class Replier {
 public:
  explicit Replier(const ReplierParams& params)
      : participant_(params.domain_participant()),
        request_(participant_, params.request_topic_name()),
        reply_(participant_, params.reply_topic_name()) {
    detail::qos_c q(params);
    rdds_replier* r = nullptr;
    rustorb::dds::detail::check(rdds_replier_new(
        participant_.rustorb_impl()->get(), request_.rustorb_impl()->get(), reply_.rustorb_impl()->get(), q.wp, q.rp,
        params.instance_name().empty() ? nullptr : params.instance_name().c_str(), &r));
    r_ = std::shared_ptr<rdds_replier>(r, rdds_replier_release);
  }

  // Waits for a request; false after max_wait. `id` gets what the reply
  // names.
  bool receive_request(TReq& request, SampleIdentity& id, const ::dds::core::Duration& max_wait) {
    rdds_in* in = nullptr;
    rdds_sample_identity c;
    rdds_status st = rdds_replier_receive_request(r_.get(), max_wait.c(), &in, &c);
    if (st == RDDS_RETCODE_TIMEOUT) return false;
    rustorb::dds::detail::check(st);
    rustorb::dds::in_stream s(in);
    rustorb::dds::read_sample(s.get(), request);
    id = SampleIdentity(c);
    return true;
  }

  // Sends `reply` to the request `id`.
  void send_reply(const TRep& reply, const SampleIdentity& id) {
    rustorb::dds::out_stream s;
    rustorb::dds::write_sample(s, reply);
    rustorb::dds::detail::check(rdds_replier_send_reply(r_.get(), s.get(), &id.c()));
  }

  ::dds::domain::DomainParticipant domain_participant() const { return participant_; }

 private:
  ::dds::domain::DomainParticipant participant_;
  ::dds::topic::Topic<TReq> request_;
  ::dds::topic::Topic<TRep> reply_;
  std::shared_ptr<rdds_replier> r_;
};

}  // namespace rpc
}  // namespace dds

#endif  // RUSTORB_DDS_RPC_HPP

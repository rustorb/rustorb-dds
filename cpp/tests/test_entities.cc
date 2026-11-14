// The entities of the C++ binding on the loopback interface: publication
// and subscription, instances, listeners, conditions and WaitSets, QoS
// matching, and the errors.

#include "Generated_dds.hh"
#include "HelloWorld_dds.hh"
#include "Keyed_dds.hh"

#include <dds/dds.hpp>

#include <atomic>
#include <chrono>
#include <thread>

#include "check.hpp"

namespace {

namespace policy = dds::core::policy;
using dds::core::Duration;
using dds::sub::status::DataState;
using dds::sub::status::InstanceState;

// Each test has its own domain, so that they do not see each other.
dds::domain::DomainParticipant participant(uint32_t domain) { return rustorb::dds::localhost_participant(domain); }

dds::pub::qos::DataWriterQos reliable_writer(const dds::pub::Publisher& pub) {
  dds::pub::qos::DataWriterQos q = pub.default_datawriter_qos();
  q << policy::Reliability::Reliable() << policy::History::KeepAll();
  return q;
}

dds::sub::qos::DataReaderQos reliable_reader(const dds::sub::Subscriber& sub) {
  dds::sub::qos::DataReaderQos q = sub.default_datareader_qos();
  q << policy::Reliability::Reliable() << policy::History::KeepAll();
  return q;
}

template <typename F>
bool eventually(F f, int ms = 5000) {
  auto end = std::chrono::steady_clock::now() + std::chrono::milliseconds(ms);
  while (std::chrono::steady_clock::now() < end) {
    if (f()) return true;
    std::this_thread::sleep_for(std::chrono::milliseconds(10));
  }
  return f();
}

}  // namespace

TEST(publish_and_take_in_order) {
  auto dp = participant(70);
  dds::topic::Topic<HelloWorld::Greeting> topic(dp, "greetings");
  CHECK(topic.name() == "greetings");
  CHECK(topic.type_name() == "HelloWorld::Greeting");
  CHECK(topic.domain_participant() == dp);
  dds::pub::Publisher pub(dp);
  dds::sub::Subscriber sub(dp);
  dds::sub::DataReader<HelloWorld::Greeting> reader(sub, topic, reliable_reader(sub));
  dds::pub::DataWriter<HelloWorld::Greeting> writer(pub, topic, reliable_writer(pub));
  CHECK(writer.wait_for_subscriptions(1, Duration(5)));
  CHECK(reader.wait_for_publications(1, Duration(5)));
  CHECK(writer.matched_subscriptions().size() == 1);
  CHECK(writer.matched_subscriptions()[0] == reader.instance_handle());
  CHECK(reader.matched_publications()[0] == writer.instance_handle());
  for (uint32_t i = 1; i <= 10; i++) writer << HelloWorld::Greeting(i, "hello");
  writer.write(HelloWorld::Greeting(11, "stamped"), dds::core::Time(1234, 5));
  writer.wait_for_acknowledgments(Duration(5));
  std::vector<HelloWorld::Greeting> got;
  CHECK(eventually([&] {
    for (const auto& s : reader.take()) {
      CHECK(s.info().valid());
      if (s.data().index() == 11) CHECK(s.info().timestamp() == dds::core::Time(1234, 5));
      got.push_back(s.data());
    }
    return got.size() == 11;
  }));
  for (uint32_t i = 0; i < 10; i++) CHECK(got[i] == HelloWorld::Greeting(i + 1, "hello"));
  CHECK(reader.take().length() == 0);
  auto status = writer.publication_matched_status();
  CHECK(status.current_count() == 1);
  CHECK(status.last_subscription_handle() == reader.instance_handle());
  CHECK(reader.subscription_matched_status().total_count() == 1);
}

TEST(read_marks_samples_read) {
  auto dp = participant(71);
  dds::topic::Topic<HelloWorld::Greeting> topic(dp, "greetings");
  dds::pub::Publisher pub(dp);
  dds::sub::Subscriber sub(dp);
  dds::sub::DataReader<HelloWorld::Greeting> reader(sub, topic, reliable_reader(sub));
  dds::pub::DataWriter<HelloWorld::Greeting> writer(pub, topic, reliable_writer(pub));
  CHECK(writer.wait_for_subscriptions(1, Duration(5)));
  writer << HelloWorld::Greeting(1, "a") << HelloWorld::Greeting(2, "b");
  CHECK(eventually([&] { return reader.read().length() == 2; }));
  CHECK(reader.select().state(DataState::new_data()).read().length() == 0);
  auto read = reader.select().state(dds::sub::status::SampleState::read()).max_samples(1).read();
  CHECK(read.length() == 1);
  CHECK(read[0].info().state().sample_state() == dds::sub::status::SampleState::read());
  // max_samples takes the first to arrive and leaves the others.
  auto first = reader.select().max_samples(1).take();
  CHECK(first.length() == 1 && first[0].data().index() == 1);
  CHECK(reader.select().max_samples(0).take().length() == 0);
  // Written in this participant: in the reader when write returns.
  writer << HelloWorld::Greeting(3, "c") << HelloWorld::Greeting(4, "d");
  auto not_read = DataState(dds::sub::status::SampleState::not_read(), dds::sub::status::ViewState::any(),
                            dds::sub::status::InstanceState::any());
  auto three = reader.select().state(not_read).max_samples(1).read();
  CHECK(three.length() == 1 && three[0].data().index() == 3);
  // Only the sample read is marked read.
  auto four = reader.select().state(not_read).read();
  CHECK(four.length() == 1 && four[0].data().index() == 4);
  CHECK(reader.take().length() == 3);
}

TEST(instances) {
  auto dp = participant(72);
  dds::topic::Topic<Keyed::Shape> topic(dp, "shapes");
  dds::pub::Publisher pub(dp);
  dds::sub::Subscriber sub(dp);
  dds::sub::DataReader<Keyed::Shape> reader(sub, topic, reliable_reader(sub));
  auto wq = reliable_writer(pub);
  wq << policy::WriterDataLifecycle::ManuallyDisposeUnregisteredInstances();
  dds::pub::DataWriter<Keyed::Shape> writer(pub, topic, wq);
  CHECK(writer.wait_for_subscriptions(1, Duration(5)));

  auto h1 = writer.register_instance(Keyed::Shape(1, 0, ""));
  CHECK(!h1.is_nil());
  CHECK(writer.lookup_instance(Keyed::Shape(1, 99, "other")) == h1);
  CHECK(writer.lookup_instance(Keyed::Shape(9, 0, "")).is_nil());
  Keyed::Shape key;
  CHECK(writer.key_value(key, h1).id() == 1);
  for (uint32_t i = 1; i <= 6; i++) writer << Keyed::Shape((i - 1) % 3 + 1, i, "s");
  CHECK(eventually([&] { return reader.read().length() == 6; }));
  auto h2 = reader.lookup_instance(Keyed::Shape(2, 0, ""));
  CHECK(!h2.is_nil());
  CHECK(reader.key_value(key, h2).id() == 2);
  auto two = reader.select().instance(h2).take();
  CHECK(two.length() == 2);
  for (const auto& s : two) CHECK(s.data().id() == 2 && s.info().instance_handle() == h2);
  // next_instance goes through the others.
  size_t seen = 0;
  dds::core::InstanceHandle h;
  for (;;) {
    auto next = reader.select().next_instance(h).take();
    if (next.length() == 0) break;
    h = next[0].info().instance_handle();
    seen += next.length();
  }
  CHECK(seen == 4);

  writer.dispose_instance(h1);
  writer.unregister_instance(writer.lookup_instance(Keyed::Shape(3, 0, "")));
  CHECK_THROWS(writer.dispose_instance(dds::core::InstanceHandle()), dds::core::PreconditionNotMetError);
  bool disposed = false, no_writers = false;
  CHECK(eventually([&] {
    for (const auto& s : reader.take()) {
      if (s.info().valid()) continue;
      auto st = s.info().state().instance_state();
      disposed |= s.data().id() == 1 && st == InstanceState::not_alive_disposed();
      no_writers |= s.data().id() == 3 && st == InstanceState::not_alive_no_writers();
    }
    return disposed && no_writers;
  }));
  writer.dispose_instance(Keyed::Shape(2, 0, ""));
}

struct WriterListener : dds::pub::NoOpDataWriterListener<HelloWorld::Greeting> {
  std::atomic<int> matched{0};
  std::atomic<int> incompatible{0};
  std::atomic<bool> got_writer{false};
  // No CHECK here: an exception must not leave a listener.
  void on_publication_matched(dds::pub::DataWriter<HelloWorld::Greeting>& w,
                              const dds::core::status::PublicationMatchedStatus& s) override {
    got_writer = w != dds::core::null;
    matched = s.current_count();
  }
  void on_offered_incompatible_qos(dds::pub::DataWriter<HelloWorld::Greeting>&,
                                   const dds::core::status::OfferedIncompatibleQosStatus& s) override {
    incompatible = s.total_count();
  }
};

struct ReaderListener : dds::sub::NoOpDataReaderListener<HelloWorld::Greeting> {
  std::atomic<int> matched{0};
  std::atomic<int> taken{0};
  void on_subscription_matched(dds::sub::DataReader<HelloWorld::Greeting>&,
                               const dds::core::status::SubscriptionMatchedStatus& s) override {
    matched = s.current_count();
  }
  void on_data_available(dds::sub::DataReader<HelloWorld::Greeting>& r) override {
    taken += static_cast<int>(r.take().length());
  }
};

TEST(listeners) {
  auto dp = participant(73);
  dds::topic::Topic<HelloWorld::Greeting> topic(dp, "greetings");
  dds::pub::Publisher pub(dp);
  dds::sub::Subscriber sub(dp);
  ReaderListener rl;
  dds::sub::DataReader<HelloWorld::Greeting> reader(sub, topic, reliable_reader(sub), &rl);
  CHECK(reader.listener() == &rl);
  WriterListener wl;
  dds::pub::DataWriter<HelloWorld::Greeting> writer(pub, topic, reliable_writer(pub), &wl,
                                                    dds::core::status::StatusMask::publication_matched());
  CHECK(eventually([&] { return wl.matched == 1 && rl.matched == 1; }));
  CHECK(wl.got_writer);
  for (uint32_t i = 1; i <= 5; i++) writer << HelloWorld::Greeting(i, "l");
  CHECK(eventually([&] { return rl.taken == 5; }));
  // Cleared: no more calls.
  reader.listener(nullptr, dds::core::status::StatusMask::none());
  writer << HelloWorld::Greeting(6, "l");
  CHECK(eventually([&] { return reader.read().length() == 1; }));
  CHECK(rl.taken == 5);

  // A best-effort writer for a reliable reader: incompatible.
  WriterListener wl2;
  dds::pub::qos::DataWriterQos be = pub.default_datawriter_qos();
  be << policy::Reliability::BestEffort();
  dds::pub::DataWriter<HelloWorld::Greeting> w2(pub, topic, be, &wl2);
  CHECK(eventually([&] { return wl2.incompatible >= 1; }));
  CHECK(eventually([&] { return reader.requested_incompatible_qos_status().total_count() >= 1; }));
}

TEST(waitsets_and_conditions) {
  auto dp = participant(74);
  dds::topic::Topic<HelloWorld::Greeting> topic(dp, "greetings");
  dds::pub::Publisher pub(dp);
  dds::sub::Subscriber sub(dp);
  dds::sub::DataReader<HelloWorld::Greeting> reader(sub, topic, reliable_reader(sub));
  dds::pub::DataWriter<HelloWorld::Greeting> writer(pub, topic, reliable_writer(pub));

  dds::core::cond::WaitSet ws;
  dds::core::cond::GuardCondition guard;
  dds::core::cond::StatusCondition status(reader);
  status.enabled_statuses(dds::core::status::StatusMask::data_available());
  CHECK(status.enabled_statuses() == dds::core::status::StatusMask::data_available());
  dds::sub::cond::ReadCondition rc(reader, DataState::new_data());
  ws += guard;
  ws += status;
  ws.attach_condition(rc);
  CHECK(ws.conditions().size() == 3);
  CHECK_THROWS(ws.wait(Duration::from_millisecs(50)), dds::core::TimeoutError);

  guard.trigger_value(true);
  auto active = ws.wait(Duration(1));
  CHECK(active.size() == 1 && active[0] == guard);
  int handled = 0;
  guard.handler([&] { handled++; });
  ws.dispatch(Duration(1));
  CHECK(handled == 1);
  guard.trigger_value(false);
  ws -= guard;
  CHECK(ws.conditions().size() == 2);

  CHECK(writer.wait_for_subscriptions(1, Duration(5)));
  writer << HelloWorld::Greeting(1, "w");
  CHECK(eventually([&] { return ws.wait(Duration(1)).size() == 2; }));
  auto samples = reader.select().condition(rc).take();
  CHECK(samples.length() == 1 && samples[0].data().index() == 1);
  CHECK(!rc.trigger_value());
}

TEST(partitions) {
  auto dp = participant(75);
  dds::topic::Topic<HelloWorld::Greeting> topic(dp, "greetings");
  dds::pub::qos::PublisherQos pq;
  pq << policy::Partition("left");
  dds::pub::Publisher left(dp, pq);
  dds::sub::qos::SubscriberQos sq;
  sq << policy::Partition(dds::core::StringSeq{"ri*"});
  dds::sub::Subscriber right(dp, sq);
  dds::sub::DataReader<HelloWorld::Greeting> reader(right, topic);
  dds::pub::DataWriter<HelloWorld::Greeting> w1(left, topic);
  CHECK(!w1.wait_for_subscriptions(1, Duration::from_millisecs(300)));
  pq << policy::Partition("right");
  dds::pub::Publisher matching(dp, pq);
  dds::pub::DataWriter<HelloWorld::Greeting> w2(matching, topic);
  CHECK(w2.wait_for_subscriptions(1, Duration(5)));
  CHECK(matching.qos().policy<policy::Partition>().name()[0] == "right");
}

TEST(complex_type_over_xcdr2) {
  auto dp = participant(76);
  dds::topic::Topic<Gen::Top> topic(dp, "top");
  dds::pub::Publisher pub(dp);
  dds::sub::Subscriber sub(dp);
  auto rq = reliable_reader(sub);
  dds::sub::DataReader<Gen::Top> reader(sub, topic, rq);
  auto wq = reliable_writer(pub);
  wq << policy::DataRepresentation({policy::XCDR2_DATA_REPRESENTATION});
  dds::pub::DataWriter<Gen::Top> writer(pub, topic, wq);
  CHECK(writer.wait_for_subscriptions(1, Duration(5)));
  Gen::Top top;
  top.id(3);
  top.all().name("everything");
  top.all().inners({Gen::Inner(1, "one")});
  top.all().opt(5);
  writer << top;
  CHECK(eventually([&] {
    auto s = reader.take();
    if (s.length() == 0) return false;
    CHECK(s[0].data() == top);
    return true;
  }));
}

TEST(qos_values) {
  dds::pub::qos::DataWriterQos w;
  CHECK(w.policy<policy::Reliability>().kind() == policy::ReliabilityKind::RELIABLE);
  dds::sub::qos::DataReaderQos r;
  CHECK(r.policy<policy::Reliability>().kind() == policy::ReliabilityKind::BEST_EFFORT);
  CHECK(r.policy<policy::DataRepresentation>().value().size() == 2);
  dds::topic::qos::TopicQos t;
  t << policy::Durability::TransientLocal() << policy::History::KeepLast(5);
  dds::pub::qos::DataWriterQos from_topic = t;
  CHECK(from_topic.policy<policy::Durability>() == policy::Durability::TransientLocal());
  CHECK(from_topic.policy<policy::History>().depth() == 5);
  policy::History h;
  from_topic >> h;
  CHECK(h.kind() == policy::HistoryKind::KEEP_LAST);
  CHECK(Duration::from_millisecs(1500) == Duration(1, 500000000));
  CHECK(Duration(1) + Duration(0, 999999999) == Duration(1, 999999999));
  CHECK(Duration(1, 600000000) + Duration(0, 500000000) == Duration(2, 100000000));
  CHECK(Duration(2, 100000000) - Duration(0, 500000000) == Duration(1, 600000000));
  CHECK(Duration::infinite().is_infinite());
  CHECK(dds::core::Time::invalid().is_invalid());
}

TEST(errors) {
  dds::pub::DataWriter<HelloWorld::Greeting> none(dds::core::null);
  CHECK(none == dds::core::null);
  CHECK_THROWS(none << HelloWorld::Greeting(), dds::core::NullReferenceError);
  auto dp = participant(77);
  dds::topic::Topic<HelloWorld::Greeting> topic(dp, "greetings");
  dds::pub::Publisher pub(dp);
  dds::pub::DataWriter<HelloWorld::Greeting> writer(pub, topic);
  writer.close();
  CHECK_THROWS(writer << HelloWorld::Greeting(), dds::core::AlreadyClosedError);
}

struct GroupListener : dds::domain::NoOpDomainParticipantListener {
  std::atomic<int> publication_matched{0};
  std::atomic<int> data_on_readers{0};
  std::atomic<int> data_available{0};
  std::atomic<bool> right_topic{false};
  void on_publication_matched(dds::pub::AnyDataWriter& w,
                              const dds::core::status::PublicationMatchedStatus&) override {
    right_topic = w.topic_name() == "grouped" && w.type_name() == "HelloWorld::Greeting";
    publication_matched++;
  }
  void on_data_on_readers(dds::sub::Subscriber& s) override {
    data_on_readers++;
    s.notify_datareaders();
  }
  void on_data_available(dds::sub::AnyDataReader&) override { data_available++; }
};

struct TopicListener : dds::topic::NoOpTopicListener<HelloWorld::Greeting> {};

// Statuses that the writers' and readers' own listeners do not take go to
// the listeners of their publisher, subscriber or participant.
TEST(group_listeners) {
  GroupListener pl;
  auto dp = participant(79);
  dp.listener(&pl, dds::core::status::StatusMask::publication_matched());
  CHECK(dp.listener() == &pl);
  dds::topic::Topic<HelloWorld::Greeting> topic(dp, "grouped");
  TopicListener tl;
  topic.listener(&tl, dds::core::status::StatusMask::inconsistent_topic());
  CHECK(topic.listener() == &tl);
  dds::pub::Publisher pub(dp);
  GroupListener sl;
  dds::sub::Subscriber sub(dp, dp.default_subscriber_qos(), &sl,
                           dds::core::status::StatusMask::data_on_readers());
  CHECK(sub.listener() == &sl);
  ReaderListener rl;
  dds::sub::DataReader<HelloWorld::Greeting> reader(sub, topic, reliable_reader(sub), &rl,
                                                    dds::core::status::StatusMask::data_available());
  dds::pub::DataWriter<HelloWorld::Greeting> writer(pub, topic, reliable_writer(pub));
  CHECK(eventually([&] { return pl.publication_matched == 1; }));
  CHECK(pl.right_topic);
  writer << HelloWorld::Greeting(1, "g");
  // DATA_ON_READERS first; its notify_datareaders calls the reader's own.
  CHECK(eventually([&] { return sl.data_on_readers == 1 && rl.taken == 1; }));
  CHECK(sl.data_available == 0);

  // Without the subscriber's listener, the participant's takes
  // DATA_AVAILABLE for a reader that has none.
  sub.listener(nullptr, dds::core::status::StatusMask::none());
  dp.listener(&pl, dds::core::status::StatusMask::data_available());
  dds::sub::DataReader<HelloWorld::Greeting> plain(sub, topic, reliable_reader(sub));
  CHECK(eventually([&] { return writer.publication_matched_status().current_count() == 2; }));
  writer << HelloWorld::Greeting(2, "g");
  CHECK(eventually([&] { return pl.data_available >= 1 && rl.taken == 2; }));
  dp.listener(nullptr, dds::core::status::StatusMask::none());
}

TEST(content_filtered_topic) {
  auto dp = participant(80);
  dds::topic::Topic<HelloWorld::Greeting> topic(dp, "filtered_greetings");
  dds::topic::Filter filter("index > %0 AND message LIKE 'h%'");
  filter.add_parameter("3");
  dds::topic::ContentFilteredTopic<HelloWorld::Greeting> cft(topic, "big_greetings", filter);
  CHECK(cft.name() == "big_greetings");
  CHECK(cft.type_name() == "HelloWorld::Greeting");
  CHECK(cft.topic() == topic);
  CHECK(cft.filter_expression() == "index > %0 AND message LIKE 'h%'");
  CHECK(cft.filter_parameters().size() == 1);
  dds::pub::Publisher pub(dp);
  dds::sub::Subscriber sub(dp);
  dds::sub::DataReader<HelloWorld::Greeting> reader(sub, cft, reliable_reader(sub));
  dds::pub::DataWriter<HelloWorld::Greeting> writer(pub, topic, reliable_writer(pub));
  CHECK(writer.wait_for_subscriptions(1, Duration(5)));
  for (uint32_t i = 1; i <= 6; i++) writer << HelloWorld::Greeting(i, i == 5 ? "bye" : "hello");
  writer.wait_for_acknowledgments(Duration(5));
  std::vector<uint32_t> got;
  CHECK(eventually([&] {
    for (const auto& s : reader.take()) got.push_back(s.data().index());
    return got.size() >= 2;
  }));
  CHECK(got == std::vector<uint32_t>({4, 6}));
  std::vector<std::string> params{"5"};
  cft.filter_parameters(params.begin(), params.end());
  CHECK(cft.filter_parameters() == params);
  for (uint32_t i = 1; i <= 7; i++) writer << HelloWorld::Greeting(i, "hello");
  writer.wait_for_acknowledgments(Duration(5));
  got.clear();
  CHECK(eventually([&] {
    for (const auto& s : reader.take()) got.push_back(s.data().index());
    return got.size() >= 2;
  }));
  CHECK(got == std::vector<uint32_t>({6, 7}));
  bool threw = false;
  try {
    dds::topic::ContentFilteredTopic<HelloWorld::Greeting> bad(topic, "bad", dds::topic::Filter("nope = 1"));
  } catch (const dds::core::Exception&) {
    threw = true;
  }
  CHECK(threw);
}

TEST(requester_and_replier) {
  auto dp = participant(81);
  dds::rpc::ReplierParams rp(dp);
  rp.service_name("Echo").instance_name("echo");
  dds::rpc::Replier<HelloWorld::Greeting, HelloWorld::Greeting> replier(rp);
  dds::rpc::Requester<HelloWorld::Greeting, HelloWorld::Greeting> requester(
      dds::rpc::RequesterParams(dp).service_name("Echo"));
  CHECK(requester.wait_for_service(Duration(5)));
  std::thread server([&] {
    for (int i = 0; i < 2; i++) {
      HelloWorld::Greeting g;
      dds::rpc::SampleIdentity id;
      if (!replier.receive_request(g, id, Duration(5))) return;
      replier.send_reply(HelloWorld::Greeting(g.index() * 10, "re: " + g.message()), id);
    }
  });
  auto first = requester.send_request(HelloWorld::Greeting(1, "one"));
  auto second = requester.request(HelloWorld::Greeting(2, "two"), Duration(5));
  CHECK(second.index() == 20);
  CHECK(second.message() == "re: two");
  HelloWorld::Greeting g;
  CHECK(requester.receive_reply(g, first, Duration(5)));
  CHECK(g.index() == 10);
  CHECK(!requester.receive_reply(g, first, Duration(0, 100000000)));
  server.join();
}

// DDS Security from the PROPERTY of the DomainParticipantQos: the two
// participants authenticate each other, and a protected topic (Square*
// in the governance) goes encrypted.
dds::domain::qos::DomainParticipantQos secure_qos(const std::string& who) {
  const std::string certs = std::string("file:") + RUSTORB_DDS_CERTS + "/";
  dds::domain::qos::DomainParticipantQos q;
  policy::Property p;
  p.set("dds.sec.auth.identity_ca", certs + "identity_ca.pem")
      .set("dds.sec.auth.identity_certificate", certs + who + "_cert.pem")
      .set("dds.sec.auth.private_key", certs + who + "_key.pem")
      .set("dds.sec.access.permissions_ca", certs + "permissions_ca.pem")
      .set("dds.sec.access.governance", certs + "governance.p7s")
      .set("dds.sec.access.permissions", certs + "permissions_" + who + ".p7s");
  q << p;
  return q;
}

TEST(security_from_property) {
  // Not all of the dds.sec properties: refused.
  auto partial = secure_qos("p1");
  policy::Property p;
  p.set("dds.sec.auth.identity_ca", "file:/nowhere.pem");
  partial << p;
  CHECK_THROWS(rustorb::dds::localhost_participant(82, partial), dds::core::Error);

  auto p1 = rustorb::dds::localhost_participant(82, secure_qos("p1"));
  auto p2 = rustorb::dds::localhost_participant(82, secure_qos("p2"));
  dds::topic::Topic<HelloWorld::Greeting> t1(p1, "Square_cpp");
  dds::topic::Topic<HelloWorld::Greeting> t2(p2, "Square_cpp");
  dds::pub::Publisher pub(p1);
  dds::sub::Subscriber sub(p2);
  dds::pub::DataWriter<HelloWorld::Greeting> w(pub, t1, reliable_writer(pub));
  dds::sub::qos::DataReaderQos rq = sub.default_datareader_qos();
  rq << policy::Reliability::Reliable() << policy::History::KeepAll();
  dds::sub::DataReader<HelloWorld::Greeting> r(sub, t2, rq);
  CHECK(w.wait_for_subscriptions(1, Duration(10)));
  HelloWorld::Greeting g;
  g.index(1);
  g.message("secret");
  w.write(g);
  std::vector<std::string> got;
  for (int i = 0; i < 100 && got.empty(); i++) {
    for (const auto& s : r.take()) {
      if (s.info().valid()) got.push_back(s.data().message());
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(50));
  }
  CHECK(got.size() == 1 && got[0] == "secret");
}

int main() { return check::run_all(); }

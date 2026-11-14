// Hello world over DDS, from C++ (the ISO/IEC C++ PSM of rustorb-dds).
//
// In one terminal: hello_world sub
// In another:      hello_world pub
//
// The same as the Rust example hello_world (topic "hello_world" of
// HelloWorld::Greeting) and the same command line, used by interop/run.sh:
//
//   hello_world pub <domain> <count> <interval_ms> [best_effort|reliable|transient_local]
//   hello_world sub <domain> <count> <timeout_s> [best_effort|reliable|transient_local]
//
// The subscriber exits once it has count samples (all of them, in order,
// when reliable), with an error on timeout. With RUSTORB_DDS_LOCALHOST set,
// only the loopback interface is used. RUSTORB_DDS_SECURITY=<file> turns on
// DDS Security, as in the Rust example: the file has the dds.sec.* properties
// of the participant QoS, one name=value a line.
//
// HelloWorld_dds.hh is what `rustorb-idl -bc++11 -Wbdds_only` writes for
// HelloWorld.idl.

#include "HelloWorld_dds.hh"

#include <dds/dds.hpp>

#include <chrono>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <string>
#include <thread>

namespace {

const char* const MESSAGE = "Hello, world!";

dds::domain::DomainParticipant participant(uint32_t domain) {
  dds::domain::qos::DomainParticipantQos qos;
  if (const char* path = std::getenv("RUSTORB_DDS_SECURITY")) {
    std::ifstream in(path);
    if (!in) throw dds::core::Error(std::string("cannot read ") + path);
    dds::core::policy::Property props;
    for (std::string line; std::getline(in, line);) {
      auto eq = line.find('=');
      if (line.empty() || line[0] == '#' || eq == std::string::npos) continue;
      props.set(line.substr(0, eq), line.substr(eq + 1));
    }
    qos << props;
  }
  if (std::getenv("RUSTORB_DDS_LOCALHOST")) return rustorb::dds::localhost_participant(domain, qos);
  return dds::domain::DomainParticipant(domain, qos);
}

int fail(const std::string& why) {
  std::cerr << "rustorb-dds (C++): " << why << std::endl;
  return 1;
}

}  // namespace

int main(int argc, char** argv) {
  std::string role = argc > 1 ? argv[1] : "pub";
  uint32_t domain = argc > 2 ? static_cast<uint32_t>(std::stoul(argv[2])) : 0;
  uint32_t count = argc > 3 ? static_cast<uint32_t>(std::stoul(argv[3])) : 10;
  std::string kind = argc > 5 ? argv[5] : "best_effort";
  if (kind != "best_effort" && kind != "reliable" && kind != "transient_local")
    return fail("unknown reliability " + kind);
  bool reliable = kind != "best_effort";
  bool transient_local = kind == "transient_local";

  namespace policy = dds::core::policy;
  try {
    dds::domain::DomainParticipant dp = participant(domain);
    dds::topic::Topic<HelloWorld::Greeting> topic(dp, "hello_world");
    if (role == "pub") {
      int interval = argc > 4 ? std::stoi(argv[4]) : 1000;
      dds::pub::Publisher pub(dp);
      dds::pub::qos::DataWriterQos qos = pub.default_datawriter_qos();
      if (reliable) {
        qos << policy::Reliability::Reliable() << policy::History::KeepAll()
            << (transient_local ? policy::Durability::TransientLocal() : policy::Durability::Volatile());
      } else {
        qos << policy::Reliability::BestEffort();
      }
      dds::pub::DataWriter<HelloWorld::Greeting> writer(pub, topic, qos);
      if (!transient_local) {
        std::cout << "rustorb-dds (C++): waiting for a subscriber..." << std::endl;
        while (!writer.wait_for_subscriptions(1, dds::core::Duration(1))) {
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(500));
      }
      for (uint32_t i = 1; i <= count; i++) {
        writer << HelloWorld::Greeting(i, MESSAGE);
        std::cout << "rustorb-dds (C++): sent " << i << std::endl;
        std::this_thread::sleep_for(std::chrono::milliseconds(interval));
      }
      if (transient_local) {
        while (!writer.wait_for_subscriptions(1, dds::core::Duration(1))) {
        }
      }
      if (reliable) {
        writer.wait_for_acknowledgments(dds::core::Duration(30));
        std::cout << "rustorb-dds (C++): all acknowledged" << std::endl;
      }
    } else if (role == "sub") {
      int timeout = argc > 4 ? std::stoi(argv[4]) : 3600;
      auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(timeout);
      dds::sub::Subscriber sub(dp);
      dds::sub::qos::DataReaderQos qos = sub.default_datareader_qos();
      qos << (reliable ? policy::Reliability::Reliable() : policy::Reliability::BestEffort())
          << policy::History::KeepAll()
          << (transient_local ? policy::Durability::TransientLocal() : policy::Durability::Volatile());
      if (transient_local) std::this_thread::sleep_for(std::chrono::seconds(2));
      dds::sub::DataReader<HelloWorld::Greeting> reader(sub, topic, qos);
      dds::core::cond::StatusCondition status(reader);
      status.enabled_statuses(dds::core::status::StatusMask::data_available());
      dds::core::cond::WaitSet waitset;
      waitset += status;
      std::cout << "rustorb-dds (C++): waiting for greetings..." << std::endl;
      uint32_t got = 0;
      while (got < count) {
        auto left = std::chrono::duration_cast<std::chrono::milliseconds>(deadline - std::chrono::steady_clock::now());
        try {
          waitset.wait(dds::core::Duration::from_millisecs(left.count() > 0 ? left.count() : 0));
        } catch (const dds::core::TimeoutError&) {
          return fail("got " + std::to_string(got) + " of " + std::to_string(count) + " samples");
        }
        for (const auto& s : reader.take()) {
          if (!s.info().valid()) continue;
          if (s.data().message() != MESSAGE)
            return fail("bad message " + s.data().message() + " for " + std::to_string(s.data().index()));
          std::cout << "rustorb-dds (C++): received " << s.data().index() << std::endl;
          got++;
          if (reliable && s.data().index() != got)
            return fail("expected " + std::to_string(got) + ", got " + std::to_string(s.data().index()));
        }
      }
    } else {
      return fail("unknown role " + role + ": use pub or sub");
    }
  } catch (const dds::core::Exception& e) {
    return fail(e.what());
  }
  return 0;
}

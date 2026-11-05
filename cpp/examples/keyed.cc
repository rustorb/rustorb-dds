// Keyed instances over DDS, from C++: write, dispose and unregister.
//
// The same as the Rust example keyed (topic "keyed" of Keyed::Shape, keyed
// by id) and the same command line, used by interop/run.sh:
//
//   keyed pub <domain> <count> <interval_ms>
//   keyed sub <domain> <count> <timeout_s>
//
// The publisher writes count samples to the instances id 1, 2, 3 in turn,
// then disposes of id 2 and unregisters id 3. The subscriber exits once it
// has count samples, each instance's in order, and has seen id 2
// NOT_ALIVE_DISPOSED and id 3 NOT_ALIVE_NO_WRITERS. With
// RUSTORB_DDS_LOCALHOST set, only the loopback interface is used.

#include "Keyed_dds.hh"

#include <dds/dds.hpp>

#include <chrono>
#include <cstdlib>
#include <iostream>
#include <map>
#include <string>
#include <thread>

namespace {

const char* const MESSAGE = "Hello, instance!";

dds::domain::DomainParticipant participant(uint32_t domain) {
  if (std::getenv("RUSTORB_DDS_LOCALHOST")) return rustorb::dds::localhost_participant(domain);
  return dds::domain::DomainParticipant(domain);
}

int fail(const std::string& why) {
  std::cerr << "rustorb-dds (C++): " << why << std::endl;
  return 1;
}

}  // namespace

int main(int argc, char** argv) {
  std::string role = argc > 1 ? argv[1] : "pub";
  uint32_t domain = argc > 2 ? static_cast<uint32_t>(std::stoul(argv[2])) : 0;
  uint32_t count = argc > 3 ? static_cast<uint32_t>(std::stoul(argv[3])) : 30;
  namespace policy = dds::core::policy;
  using dds::sub::status::InstanceState;
  try {
    dds::domain::DomainParticipant dp = participant(domain);
    dds::topic::Topic<Keyed::Shape> topic(dp, "keyed");
    if (role == "pub") {
      int interval = argc > 4 ? std::stoi(argv[4]) : 100;
      dds::pub::Publisher pub(dp);
      dds::pub::qos::DataWriterQos qos = pub.default_datawriter_qos();
      qos << policy::Reliability::Reliable() << policy::History::KeepAll()
          << policy::WriterDataLifecycle::ManuallyDisposeUnregisteredInstances();
      dds::pub::DataWriter<Keyed::Shape> writer(pub, topic, qos);
      std::cout << "rustorb-dds (C++): waiting for a subscriber..." << std::endl;
      while (!writer.wait_for_subscriptions(1, dds::core::Duration(1))) {
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(500));
      dds::core::InstanceHandle two = writer.register_instance(Keyed::Shape(2, 0, ""));
      for (uint32_t i = 1; i <= count; i++) {
        uint32_t id = (i - 1) % 3 + 1;
        writer.write(Keyed::Shape(id, i, MESSAGE));
        std::cout << "rustorb-dds (C++): sent id " << id << " index " << i << std::endl;
        std::this_thread::sleep_for(std::chrono::milliseconds(interval));
      }
      writer.dispose_instance(two);
      writer.unregister_instance(writer.lookup_instance(Keyed::Shape(3, 0, "")));
      std::cout << "rustorb-dds (C++): disposed 2, unregistered 3" << std::endl;
      writer.wait_for_acknowledgments(dds::core::Duration(30));
      std::cout << "rustorb-dds (C++): all acknowledged" << std::endl;
    } else if (role == "sub") {
      int timeout = argc > 4 ? std::stoi(argv[4]) : 3600;
      auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(timeout);
      dds::sub::Subscriber sub(dp);
      dds::sub::qos::DataReaderQos qos = sub.default_datareader_qos();
      qos << policy::Reliability::Reliable() << policy::History::KeepAll();
      dds::sub::DataReader<Keyed::Shape> reader(sub, topic, qos);
      std::cout << "rustorb-dds (C++): waiting for shapes..." << std::endl;
      uint32_t got = 0;
      std::map<uint32_t, uint32_t> last;
      bool disposed = false, no_writers = false;
      while (got < count || !disposed || !no_writers) {
        if (std::chrono::steady_clock::now() >= deadline)
          return fail("got " + std::to_string(got) + " of " + std::to_string(count) + " samples, disposed " +
                      std::to_string(disposed) + ", no writers " + std::to_string(no_writers));
        if (!reader.wait_for_data(dds::core::Duration::from_millisecs(200))) continue;
        for (const auto& s : reader.take()) {
          const Keyed::Shape& d = s.data();
          if (s.info().valid()) {
            if (d.message() != MESSAGE || d.index() <= last[d.id()] || (d.index() - 1) % 3 + 1 != d.id())
              return fail("bad sample id " + std::to_string(d.id()) + " index " + std::to_string(d.index()));
            last[d.id()] = d.index();
            got++;
            std::cout << "rustorb-dds (C++): received id " << d.id() << " index " << d.index() << std::endl;
          } else {
            // An invalid sample has the key fields of its instance.
            InstanceState state = s.info().state().instance_state();
            std::cout << "rustorb-dds (C++): id " << d.id() << " is " << state.to_ulong() << std::endl;
            disposed |= d.id() == 2 && state == InstanceState::not_alive_disposed();
            no_writers |= d.id() == 3 && state == InstanceState::not_alive_no_writers();
          }
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

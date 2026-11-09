// Talks with OpenRTM-aist 2.0 data ports over its Fast DDS transport,
// from C++: the same as the Rust example openrtm, with the same command
// line:
//
//   openrtm out [domain [count [interval_ms [topic]]]]
//   openrtm in [domain [count [timeout_s [topic]]]]
//
// out waits for a reader, then writes RTC::TimedLong samples with data
// 1, 2, 3...; in prints what it gets and exits after count samples, with
// an error on timeout. The topic is "chatter" by default, as in
// OpenRTM-aist.
//
// OpenRTM_dds.hh is what `rustorb-idl -bc++11 -Wbdds_only
// -Wbextensibility=final` writes for interop/idl/OpenRTM.idl (or
// OpenRTM-aist's BasicDataType.idl, which declares the same types).

#include "OpenRTM_dds.hh"

#include <dds/dds.hpp>

#include <chrono>
#include <cstdint>
#include <iomanip>
#include <iostream>
#include <string>
#include <thread>

namespace {

int fail(const std::string& why) {
  std::cerr << "rustorb-dds (C++): " << why << std::endl;
  return 1;
}

}  // namespace

int main(int argc, char** argv) {
  std::string role = argc > 1 ? argv[1] : "in";
  uint32_t domain = argc > 2 ? static_cast<uint32_t>(std::stoul(argv[2])) : 0;
  long long count = argc > 3 ? std::stoll(argv[3]) : -1;
  std::string topic_name = argc > 5 ? argv[5] : "chatter";

  namespace policy = dds::core::policy;
  try {
    dds::domain::DomainParticipant dp(domain);
    dds::topic::Topic<RTC::TimedLong> topic(dp, topic_name);
    if (role == "out") {
      int interval = argc > 4 ? std::stoi(argv[4]) : 1000;
      dds::pub::Publisher pub(dp);
      dds::pub::qos::DataWriterQos qos = pub.default_datawriter_qos();
      // OpenRTM-aist's ports by default: best effort, keep last 1.
      qos << policy::Reliability::BestEffort() << policy::History::KeepLast(1);
      dds::pub::DataWriter<RTC::TimedLong> writer(pub, topic, qos);
      while (!writer.wait_for_subscriptions(1, dds::core::Duration(1))) {
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(500));
      for (long long i = 1; count < 0 || i <= count; i++) {
        auto now = std::chrono::system_clock::now().time_since_epoch();
        auto ns = std::chrono::duration_cast<std::chrono::nanoseconds>(now).count();
        RTC::Time tm(static_cast<uint32_t>(ns / 1000000000), static_cast<uint32_t>(ns % 1000000000));
        writer << RTC::TimedLong(tm, static_cast<int32_t>(i));
        std::cout << "rustorb-dds (C++): write " << i << std::endl;
        std::this_thread::sleep_for(std::chrono::milliseconds(interval));
      }
    } else if (role == "in") {
      int timeout = argc > 4 ? std::stoi(argv[4]) : 60;
      auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(timeout);
      dds::sub::Subscriber sub(dp);
      dds::sub::qos::DataReaderQos qos = sub.default_datareader_qos();
      qos << policy::Reliability::BestEffort() << policy::History::KeepLast(1);
      dds::sub::DataReader<RTC::TimedLong> reader(sub, topic, qos);
      dds::core::cond::StatusCondition status(reader);
      status.enabled_statuses(dds::core::status::StatusMask::data_available());
      dds::core::cond::WaitSet waitset;
      waitset += status;
      long long got = 0;
      while (count < 0 || got < count) {
        auto left = std::chrono::duration_cast<std::chrono::milliseconds>(deadline - std::chrono::steady_clock::now());
        try {
          waitset.wait(dds::core::Duration::from_millisecs(left.count() > 0 ? left.count() : 0));
        } catch (const dds::core::TimeoutError&) {
          return fail("got " + std::to_string(got) + " samples");
        }
        for (const auto& s : reader.take()) {
          if (!s.info().valid()) continue;
          std::cout << "rustorb-dds (C++): read " << s.data().data() << " (tm " << s.data().tm().sec() << "."
                    << std::setw(9) << std::setfill('0') << s.data().tm().nsec() << std::setfill(' ') << ")" << std::endl;
          got++;
        }
      }
    } else {
      return fail("unknown role " + role + ": use out or in");
    }
  } catch (const dds::core::Exception& e) {
    return fail(e.what());
  }
  return 0;
}

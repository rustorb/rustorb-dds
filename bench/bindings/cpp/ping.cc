// ping of bench/bindings.py in C++ (rustorb-dds#107): the round trip
// through `rustorb-dds-perf pong`, as `rustorb-dds-perf ping` measures it.
//
//   ping <domain> <sizes> <warmup_s> <duration_s>
//
// Writes RustorbPerf::Sample on RustorbPerf_ping (reliable, KEEP_LAST 1),
// waits on a WaitSet for the sample of the same seq on RustorbPerf_pong,
// and prints one line of JSON per size.

#include "Perf_dds.hh"

#include <dds/dds.hpp>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <sstream>
#include <string>
#include <vector>

namespace {

using Clock = std::chrono::steady_clock;

double at(const std::vector<int64_t>& ns, double p) {
  if (ns.empty()) return 0;
  size_t n = ns.size();
  size_t i = std::min(std::max<size_t>(static_cast<size_t>(std::ceil(p * n)), 1), n) - 1;
  return ns[i] / 1000.0;
}

}  // namespace

int main(int argc, char** argv) {
  if (argc < 5) {
    std::fprintf(stderr, "usage: ping <domain> <sizes> <warmup_s> <duration_s>\n");
    return 2;
  }
  uint32_t domain = static_cast<uint32_t>(std::stoul(argv[1]));
  std::vector<size_t> sizes;
  std::stringstream list(argv[2]);
  for (std::string s; std::getline(list, s, ',');) sizes.push_back(std::stoul(s));
  auto warmup = std::chrono::duration<double>(std::stod(argv[3]));
  auto duration = std::chrono::duration<double>(std::stod(argv[4]));

  namespace policy = dds::core::policy;
  try {
    dds::domain::DomainParticipant dp(domain);
    dds::topic::Topic<RustorbPerf::Sample> ping_topic(dp, "RustorbPerf_ping");
    dds::topic::Topic<RustorbPerf::Sample> pong_topic(dp, "RustorbPerf_pong");
    dds::pub::Publisher pub(dp);
    dds::sub::Subscriber sub(dp);
    dds::pub::qos::DataWriterQos wq = pub.default_datawriter_qos();
    wq << policy::Reliability::Reliable() << policy::History::KeepLast(1);
    dds::sub::qos::DataReaderQos rq = sub.default_datareader_qos();
    rq << policy::Reliability::Reliable() << policy::History::KeepLast(1);
    dds::pub::DataWriter<RustorbPerf::Sample> writer(pub, ping_topic, wq);
    dds::sub::DataReader<RustorbPerf::Sample> reader(sub, pong_topic, rq);
    writer.wait_for_subscriptions(1, dds::core::Duration(30));
    dds::core::cond::StatusCondition status(reader);
    status.enabled_statuses(dds::core::status::StatusMask::data_available());
    dds::core::cond::WaitSet waitset;
    waitset += status;
    auto start = Clock::now();
    uint64_t seq = 0;
    for (size_t size : sizes) {
      RustorbPerf::Sample sample(0, 0, std::vector<uint8_t>(size));
      reader.take();
      auto warm_end = Clock::now() + std::chrono::duration_cast<Clock::duration>(warmup);
      auto end = warm_end + std::chrono::duration_cast<Clock::duration>(duration);
      std::vector<int64_t> rtts;
      uint64_t lost = 0;
      while (Clock::now() < end) {
        bool measuring = Clock::now() >= warm_end;
        seq++;
        auto sent = Clock::now();
        sample.seq(seq);
        sample.stamp(std::chrono::duration_cast<std::chrono::nanoseconds>(sent - start).count());
        writer.write(sample);
        auto deadline = sent + std::chrono::seconds(1);
        for (;;) {
          auto left = std::chrono::duration_cast<std::chrono::milliseconds>(deadline - Clock::now());
          try {
            waitset.wait(dds::core::Duration::from_millisecs(left.count() > 0 ? left.count() : 0));
          } catch (const dds::core::TimeoutError&) {
            if (measuring) lost++;
            break;
          }
          bool got = false;
          for (const auto& s : reader.take())
            if (s.info().valid() && s.data().seq() == seq) got = true;
          if (got) {
            if (measuring)
              rtts.push_back(std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now() - sent).count());
            break;
          }
        }
      }
      std::sort(rtts.begin(), rtts.end());
      std::printf(
          "{\"tool\":\"bench/bindings/cpp\",\"impl\":\"rustorb-dds-cpp\",\"mode\":\"ping\",\"size\":%zu,"
          "\"reliability\":\"reliable\",\"lost\":%llu,\"seconds\":%.3f,\"count\":%zu,\"rtt_min_us\":%.3f,"
          "\"rtt_p50_us\":%.3f,\"rtt_p90_us\":%.3f,\"rtt_p99_us\":%.3f,\"rtt_max_us\":%.3f}\n",
          size, static_cast<unsigned long long>(lost), duration.count(), rtts.size(), at(rtts, 0),
          at(rtts, 0.5), at(rtts, 0.9), at(rtts, 0.99), rtts.empty() ? 0.0 : rtts.back() / 1000.0);
      std::fflush(stdout);
    }
  } catch (const dds::core::Exception& e) {
    std::fprintf(stderr, "ping: %s\n", e.what());
    return 1;
  }
  return 0;
}

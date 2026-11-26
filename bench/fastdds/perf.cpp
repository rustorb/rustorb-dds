// The measurements of rustorb-dds-perf (crates/rustorb-dds-perf) for
// Fast DDS, on its public C++ API only, with the same command line,
// protocol and JSON output, so that bench/run.py measures both the same
// way and they can talk to each other (rustorb-dds#104).
//
//   perf ping|pong|pub|sub|disc [--domain N] [--size N | --sizes A,B,...]
//        [--reliability reliable|best-effort] [--transport udp|shm]
//        [--warmup S] [--duration S] [--count N] [--rate N] [--depth N]
//        [--reply-timeout MS] [--timeout S] [--label TEXT] [--json]
//        [--participants N] [--start T]
//
// --transport udp gives the participant the UDPv4 transport only; shm
// keeps Fast DDS's builtin transports, which go through shared memory to
// Fast DDS participants of the same host (and UDP to the others).

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <sstream>
#include <string>
#include <thread>
#include <vector>

#include <fastdds/dds/domain/DomainParticipant.hpp>
#include <fastdds/dds/domain/DomainParticipantFactory.hpp>
#include <fastdds/dds/publisher/DataWriter.hpp>
#include <fastdds/dds/publisher/Publisher.hpp>
#include <fastdds/dds/subscriber/DataReader.hpp>
#include <fastdds/dds/subscriber/SampleInfo.hpp>
#include <fastdds/dds/subscriber/Subscriber.hpp>
#include <fastdds/dds/topic/TypeSupport.hpp>
#include <fastdds/rtps/transport/UDPv4TransportDescriptor.h>
#include <fastrtps/config.h>

#include "PerfPubSubTypes.h"

using namespace eprosima::fastdds::dds;
using Clock = std::chrono::steady_clock;

enum Mode { PING, PONG, PUB, SUB, DISC };
// The samples a best-effort sub keeps not yet taken (as rustorb-dds-perf).
static const int READER_MAX_SAMPLES = 1000;

static const char* mode_name[] = {"ping", "pong", "pub", "sub", "disc"};

static struct Opts {
    Mode mode = PING;
    uint32_t domain = 0;
    std::vector<size_t> sizes;
    bool reliable = true;
    std::string transport = "shm";
    double warmup = 1, duration = 5, rate = 0, timeout = 60, reply_timeout = 1.0;
    long long count = -1;
    int depth = -1;
    std::string label;
    bool json = false;
    int participants = 2;
    double start = 0;  // seconds since the epoch, 0: this process's start
} o;

static double now_s()
{
    return std::chrono::duration<double>(Clock::now().time_since_epoch()).count();
}

static void sleep_s(double s)
{
    if (s > 0)
        std::this_thread::sleep_for(std::chrono::duration<double>(s));
}

[[noreturn]] static void usage(const char* why)
{
    std::fprintf(stderr, "%s\nusage: perf ping|pong|pub|sub [options] (see bench/fastdds/perf.cpp)\n", why);
    std::exit(2);
}

static void parse(int argc, char** argv)
{
    if (argc < 2)
        usage("no mode");
    std::string m = argv[1];
    if (m == "ping") o.mode = PING;
    else if (m == "pong") o.mode = PONG;
    else if (m == "pub") o.mode = PUB;
    else if (m == "sub") o.mode = SUB;
    else if (m == "disc") o.mode = DISC;
    else usage("unknown mode");
    for (int i = 2; i < argc; i++) {
        std::string a = argv[i];
        if (a == "--json") { o.json = true; continue; }
        if (i + 1 >= argc) usage("an option needs a value");
        std::string v = argv[++i];
        if (a == "--domain") o.domain = std::stoul(v);
        else if (a == "--size") o.sizes = {std::stoul(v)};
        else if (a == "--sizes") {
            o.sizes.clear();
            std::stringstream ss(v);
            std::string tok;
            while (std::getline(ss, tok, ','))
                o.sizes.push_back(std::stoul(tok));
        }
        else if (a == "--reliability") o.reliable = v == "reliable";
        else if (a == "--transport") o.transport = v;
        else if (a == "--warmup") o.warmup = std::stod(v);
        else if (a == "--duration") o.duration = std::stod(v);
        else if (a == "--count") o.count = std::stoll(v);
        else if (a == "--rate") o.rate = std::stod(v);
        else if (a == "--depth") o.depth = std::stoi(v);
        else if (a == "--reply-timeout") o.reply_timeout = std::stod(v) / 1000.0;
        else if (a == "--timeout") o.timeout = std::stod(v);
        else if (a == "--label") o.label = v;
        else if (a == "--participants") o.participants = std::atoi(v.c_str());
        else if (a == "--start") o.start = std::atof(v.c_str());
        else usage("unknown option");
    }
    if (o.sizes.empty()) {
        if (o.mode == PING || o.mode == PONG)
            for (int e = 5; e <= 20; e++)
                o.sizes.push_back(size_t(1) << e);
        else
            o.sizes.push_back(1024);
    }
}

static void json_head(size_t size)
{
    std::printf("{\"tool\":\"rustorb-dds-perf\",\"impl\":\"fastdds\",\"version\":\"%d.%d.%d\","
                "\"mode\":\"%s\",\"size\":%zu,\"reliability\":\"%s\",\"transport\":\"%s\"",
                FASTRTPS_VERSION_MAJOR, FASTRTPS_VERSION_MINOR, FASTRTPS_VERSION_MICRO,
                mode_name[o.mode], size, o.reliable ? "reliable" : "best-effort", o.transport.c_str());
    if (!o.label.empty())
        std::printf(",\"label\":\"%s\"", o.label.c_str());
}

static double pct(const std::vector<double>& v, double p)
{
    size_t n = v.size();
    size_t k = (size_t)std::ceil(p * (double)n);
    k = std::max<size_t>(1, std::min(k, n));
    return v[k - 1];
}

static DomainParticipant* participant;
static DataWriter* writer;
static DataReader* reader;

static void setup()
{
    bool throughput = o.mode == PUB || o.mode == SUB;
    int depth = o.depth > 0 ? o.depth : (throughput ? 100 : 1);
    const char* w = nullptr;
    const char* r = nullptr;
    switch (o.mode) {
        case PING: w = "RustorbPerf_ping"; r = "RustorbPerf_pong"; break;
        case PONG: w = "RustorbPerf_pong"; r = "RustorbPerf_ping"; break;
        case PUB: w = "RustorbPerf_data"; break;
        case SUB: r = "RustorbPerf_data"; break;
        case DISC: w = r = "RustorbPerf_disc"; break;
    }
    DomainParticipantQos pq = PARTICIPANT_QOS_DEFAULT;
    if (o.transport == "udp") {
        pq.transport().use_builtin_transports = false;
        pq.transport().user_transports.push_back(
            std::make_shared<eprosima::fastdds::rtps::UDPv4TransportDescriptor>());
    }
    participant = DomainParticipantFactory::get_instance()->create_participant(o.domain, pq);
    if (!participant) {
        std::fprintf(stderr, "create_participant failed\n");
        std::exit(1);
    }
    TypeSupport type(new RustorbPerf::SamplePubSubType());
    type.register_type(participant);
    DataWriterQos wq = DATAWRITER_QOS_DEFAULT;
    DataReaderQos rq = DATAREADER_QOS_DEFAULT;
    auto rel = o.reliable ? RELIABLE_RELIABILITY_QOS : BEST_EFFORT_RELIABILITY_QOS;
    wq.reliability().kind = rel;
    wq.reliability().max_blocking_time = eprosima::fastrtps::Duration_t(0, 100000000);
    rq.reliability().kind = rel;
    if (throughput && o.reliable) {
        // Flow control: a write blocks once depth samples are unacknowledged.
        wq.history().kind = KEEP_ALL_HISTORY_QOS;
        wq.resource_limits().max_samples = depth;
        wq.resource_limits().max_samples_per_instance = depth;
        wq.resource_limits().allocated_samples = depth;
    } else {
        wq.history().kind = KEEP_LAST_HISTORY_QOS;
        wq.history().depth = depth;
    }
    if (throughput) {
        rq.history().kind = KEEP_ALL_HISTORY_QOS;
        if (!o.reliable) {
            // A best-effort sub slower than its pub drops what does not
            // fit (as rustorb-dds-perf; Fast DDS's default is 5000, 400
            // an instance).
            rq.resource_limits().max_samples = READER_MAX_SAMPLES;
            rq.resource_limits().max_samples_per_instance = READER_MAX_SAMPLES;
        }
    } else {
        rq.history().kind = KEEP_LAST_HISTORY_QOS;
        rq.history().depth = depth;
    }
    Topic* wt = nullptr;
    if (w) {
        wt = participant->create_topic(w, type.get_type_name(), TOPIC_QOS_DEFAULT);
        writer = participant->create_publisher(PUBLISHER_QOS_DEFAULT)->create_datawriter(wt, wq);
    }
    if (r) {
        Topic* t = wt && std::string(w) == r
                ? wt
                : participant->create_topic(r, type.get_type_name(), TOPIC_QOS_DEFAULT);
        reader = participant->create_subscriber(SUBSCRIBER_QOS_DEFAULT)->create_datareader(t, rq);
    }
    if ((w && !writer) || (r && !reader)) {
        std::fprintf(stderr, "creating the endpoints failed\n");
        std::exit(1);
    }
}

static int32_t writer_matched()
{
    PublicationMatchedStatus st;
    if (writer)
        writer->get_publication_matched_status(st);
    return writer ? st.current_count : 0;
}

static int32_t reader_matched()
{
    SubscriptionMatchedStatus st;
    if (reader)
        reader->get_subscription_matched_status(st);
    return reader ? st.current_count : 0;
}

static void wait_for_match()
{
    double deadline = now_s() + o.timeout;
    for (;;) {
        if ((!writer || writer_matched() > 0) && (!reader || reader_matched() > 0)) {
            sleep_s(0.3);
            return;
        }
        if (now_s() > deadline) {
            std::fprintf(stderr, "%s: nothing matched in %.0f s\n", mode_name[o.mode], o.timeout);
            std::exit(1);
        }
        sleep_s(0.02);
    }
}

static bool wait_data(double s)
{
    if (s <= 0)
        return false;
    int32_t sec = (int32_t)s;
    uint32_t ns = (uint32_t)((s - sec) * 1e9);
    return reader->wait_for_unread_message(eprosima::fastrtps::Duration_t(sec, ns));
}

static RustorbPerf::Sample out_sample;

static ReturnCode_t send(uint64_t seq, uint64_t stamp, size_t len)
{
    if (out_sample.payload().size() != len)
        out_sample.payload().resize(len, 0x5a);
    out_sample.seq(seq);
    out_sample.stamp(stamp);
    return writer->write(&out_sample, eprosima::fastrtps::rtps::InstanceHandle_t());
}

static void ping()
{
    wait_for_match();
    double start = now_s();
    uint64_t seq = 0;
    RustorbPerf::Sample in;
    SampleInfo info;
    if (!o.json)
        std::printf("%8s %8s %6s %9s %9s %9s %9s %9s  (round trip, us)\n", "size", "count", "lost", "p50", "p90", "p99", "p99.9", "max");
    for (size_t size : o.sizes) {
        double warm_end = now_s() + o.warmup, end = warm_end + o.duration;
        std::vector<double> rtt;
        uint64_t lost = 0;
        while (reader->take_next_sample(&in, &info) == ReturnCode_t::RETCODE_OK) {
        }
        for (;;) {
            double t = now_s();
            bool measuring = t >= warm_end;
            if (t >= end || (measuring && o.count >= 0 && (long long)rtt.size() >= o.count))
                break;
            seq++;
            double sent = now_s();
            ReturnCode_t rc = send(seq, (uint64_t)((sent - start) * 1e9), size);
            if (rc == ReturnCode_t::RETCODE_TIMEOUT) {
                if (measuring) lost++;
                continue;
            }
            if (rc != ReturnCode_t::RETCODE_OK) {
                std::fprintf(stderr, "write failed\n");
                std::exit(1);
            }
            double deadline = sent + o.reply_timeout;
            for (;;) {
                double left = deadline - now_s();
                if (left <= 0 || !wait_data(left)) {
                    if (measuring) lost++;
                    break;
                }
                bool found = false;
                while (reader->take_next_sample(&in, &info) == ReturnCode_t::RETCODE_OK)
                    if (info.valid_data && in.seq() == seq)
                        found = true;
                if (found) {
                    if (measuring)
                        rtt.push_back((now_s() - sent) * 1e6);
                    break;
                }
            }
        }
        size_t n = rtt.size();
        double mean = 0;
        for (double v : rtt) mean += v;
        mean = n ? mean / (double)n : 0;
        std::sort(rtt.begin(), rtt.end());
        double mn = n ? rtt[0] : 0, mx = n ? rtt[n - 1] : 0;
        double p50 = n ? pct(rtt, 0.5) : 0, p90 = n ? pct(rtt, 0.9) : 0;
        double p99 = n ? pct(rtt, 0.99) : 0, p999 = n ? pct(rtt, 0.999) : 0;
        if (o.json) {
            json_head(size);
            std::printf(",\"count\":%zu,\"lost\":%llu,\"seconds\":%.3f,\"rtt_min_us\":%.3f,\"rtt_mean_us\":%.3f,"
                        "\"rtt_p50_us\":%.3f,\"rtt_p90_us\":%.3f,\"rtt_p99_us\":%.3f,\"rtt_p999_us\":%.3f,\"rtt_max_us\":%.3f}\n",
                        n, (unsigned long long)lost, o.duration, mn, mean, p50, p90, p99, p999, mx);
        } else {
            std::printf("%8zu %8zu %6llu %9.1f %9.1f %9.1f %9.1f %9.1f\n", size, n, (unsigned long long)lost, p50, p90, p99, p999, mx);
        }
        std::fflush(stdout);
    }
}

static void pong()
{
    double deadline = now_s() + o.timeout;
    bool matched = false;
    uint64_t echoed = 0;
    RustorbPerf::Sample in;
    SampleInfo info;
    for (;;) {
        wait_data(0.2);
        while (reader->take_next_sample(&in, &info) == ReturnCode_t::RETCODE_OK) {
            if (!info.valid_data)
                continue;
            writer->write(&in, eprosima::fastrtps::rtps::InstanceHandle_t());
            echoed++;
        }
        int32_t w = writer_matched(), r = reader_matched();
        if (matched) {
            if (w == 0 && r == 0)
                break;
        } else if (w || r) {
            matched = true;
        } else if (now_s() > deadline) {
            std::fprintf(stderr, "pong: no ping in %.0f s\n", o.timeout);
            std::exit(1);
        }
    }
    if (!o.json)
        std::printf("pong: sent back %llu samples\n", (unsigned long long)echoed);
}

static void publish()
{
    wait_for_match();
    size_t size = o.sizes[0];
    double start = now_s(), give_up = start + o.warmup + o.duration + o.timeout;
    uint64_t seq = 0, blocked = 0;
    for (;;) {
        if (seq % 256 == 0 && (writer_matched() == 0 || now_s() >= give_up))
            break;
        ReturnCode_t rc = send(seq + 1, (uint64_t)((now_s() - start) * 1e9), size);
        if (rc == ReturnCode_t::RETCODE_OK) {
            seq++;
        } else if (rc == ReturnCode_t::RETCODE_TIMEOUT) {
            blocked++;
            if (writer_matched() == 0)
                break;
        } else {
            std::fprintf(stderr, "write failed\n");
            std::exit(1);
        }
        if (o.rate > 0)
            sleep_s(start + (double)seq / o.rate - now_s());
    }
    double secs = now_s() - start;
    if (o.json) {
        json_head(size);
        std::printf(",\"samples\":%llu,\"seconds\":%.3f,\"samples_per_s\":%.3f,\"blocked\":%llu}\n",
                    (unsigned long long)seq, secs, (double)seq / secs, (unsigned long long)blocked);
    } else {
        std::printf("pub: wrote %llu samples of %zu octets in %.1f s (%.0f/s), blocked %llu times\n",
                    (unsigned long long)seq, size, secs, (double)seq / secs, (unsigned long long)blocked);
    }
}

static void subscribe()
{
    wait_for_match();
    size_t size = o.sizes[0];
    if (!wait_data(o.timeout)) {
        std::fprintf(stderr, "sub: no sample in %.0f s\n", o.timeout);
        std::exit(1);
    }
    double warm_end = now_s() + o.warmup, end = warm_end + o.duration, second_end = warm_end + 1;
    uint64_t n = 0, octets = 0, first = 0, last = 0, in_second = 0, min_s = UINT64_MAX, max_s = 0;
    bool have_first = false;
    int seconds = 0;
    RustorbPerf::Sample in;
    SampleInfo info;
    for (;;) {
        double t = now_s();
        if (t >= end)
            break;
        if (t >= second_end && t > warm_end) {
            min_s = std::min(min_s, in_second);
            max_s = std::max(max_s, in_second);
            seconds++;
            in_second = 0;
            second_end += 1;
        }
        wait_data(std::min(end - t, 0.05));
        bool counting = now_s() >= warm_end;
        while (reader->take_next_sample(&in, &info) == ReturnCode_t::RETCODE_OK) {
            if (!counting || !info.valid_data)
                continue;
            n++;
            in_second++;
            octets += in.payload().size();
            if (!have_first) { first = in.seq(); have_first = true; }
            last = std::max(last, in.seq());
        }
    }
    if (!seconds) min_s = 0;
    uint64_t lost = have_first && last - first + 1 > n ? last - first + 1 - n : 0;
    double secs = o.duration, mbit = (double)octets * 8 / secs / 1e6;
    if (o.json) {
        json_head(size);
        std::printf(",\"samples\":%llu,\"octets\":%llu,\"lost\":%llu,\"seconds\":%.3f,\"samples_per_s\":%.3f,"
                    "\"mbit_per_s\":%.3f,\"samples_per_s_min\":%llu,\"samples_per_s_max\":%llu}\n",
                    (unsigned long long)n, (unsigned long long)octets, (unsigned long long)lost, secs,
                    (double)n / secs, mbit, (unsigned long long)min_s, (unsigned long long)max_s);
    } else {
        std::printf("sub: %llu samples of %zu octets in %.1f s: %.0f/s, %.1f Mbit/s, %llu lost\n",
                    (unsigned long long)n, size, secs, (double)n / secs, mbit, (unsigned long long)lost);
    }
}

static double wall_s()
{
    return std::chrono::duration<double>(std::chrono::system_clock::now().time_since_epoch()).count();
}

// disc: the seconds from the common start until the participant is
// created and until its writer and reader match those of all the
// participants (its own included); stays to --duration for the others.
static void discovery()
{
    double start = o.start > 0 ? o.start : wall_s();
    setup();
    double created = wall_s() - start, matched = -1;
    int32_t all = o.participants;
    while (matched < 0 && wall_s() < start + o.duration) {
        if (writer_matched() >= all && reader_matched() >= all)
            matched = wall_s() - start;
        else
            sleep_s(0.001);
    }
    // Then stays, for the others.
    sleep_s(start + o.duration - wall_s());
    if (o.json) {
        std::printf("{\"tool\":\"rustorb-dds-perf\",\"impl\":\"fastdds\",\"version\":\"%d.%d.%d\",\"mode\":\"disc\","
                    "\"transport\":\"%s\",\"participants\":%d,\"create_s\":%.6f,\"writer_matched\":%d,"
                    "\"reader_matched\":%d",
                    FASTRTPS_VERSION_MAJOR, FASTRTPS_VERSION_MINOR, FASTRTPS_VERSION_MICRO, o.transport.c_str(), o.participants, created, writer_matched(),
                    reader_matched());
        if (matched >= 0)
            std::printf(",\"match_s\":%.6f", matched);
        if (!o.label.empty())
            std::printf(",\"label\":\"%s\"", o.label.c_str());
        std::printf("}\n");
    } else if (matched >= 0) {
        std::printf("disc: participant at %.3f s, all matched at %.3f s\n", created, matched);
    } else {
        std::printf("disc: matched %d writers and %d readers of %d\n", writer_matched(), reader_matched(), all);
    }
}

int main(int argc, char** argv)
{
    parse(argc, argv);
    std::setvbuf(stdout, nullptr, _IOLBF, 0);
    if (o.mode == DISC) {
        discovery();
    } else {
        setup();
    }
    switch (o.mode) {
        case PING: ping(); break;
        case PONG: pong(); break;
        case PUB: publish(); break;
        case SUB: subscribe(); break;
        case DISC: break;
    }
    participant->delete_contained_entities();
    DomainParticipantFactory::get_instance()->delete_participant(participant);
    return 0;
}

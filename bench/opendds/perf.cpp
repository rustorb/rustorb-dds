// The measurements of rustorb-dds-perf (crates/rustorb-dds-perf) for
// OpenDDS, on its public API only (the DCPS API of DDS 1.4 in the
// classic IDL to C++ mapping), with the same command line, protocol and
// JSON output, so that bench/run.py measures both the same way and they
// can talk to each other (rustorb-dds#108). The same as
// bench/cyclonedds/perf.c.
//
//   perf ping|pong|pub|sub|disc [--domain N] [--size N | --sizes A,B,...]
//        [--reliability reliable|best-effort] [--transport udp]
//        [--warmup S] [--duration S] [--count N] [--rate N] [--depth N]
//        [--reply-timeout MS] [--timeout S] [--label TEXT] [--json]
//        [--participants N] [--start T]
//        -DCPSConfigFile bench/opendds/rtps.ini
//
// RTPS discovery and the RTPS_UDP transport (rtps.ini); writers write
// XCDR (version 1), as the others do, and readers take both versions.

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <string>
#include <thread>
#include <vector>

#include <dds/DCPS/Marked_Default_Qos.h>
#include <dds/DCPS/RTPS/RtpsDiscovery.h>
#include <dds/DCPS/Service_Participant.h>
#include <dds/DCPS/WaitSet.h>
#include <dds/DCPS/transport/rtps_udp/RtpsUdp.h>
#include <dds/DdsDcpsInfrastructureC.h>
#include <dds/Version.h>

#include "PerfTypeSupportImpl.h"

namespace {

const size_t MAX_SIZES = 64;
const int MAX_TAKE = 256;
// The samples a best-effort sub keeps not yet taken (as rustorb-dds-perf).
const int READER_MAX_SAMPLES = 1000;

enum Mode { PING, PONG, PUB, SUB, DISC };
const char* mode_name[] = {"ping", "pong", "pub", "sub", "disc"};

struct Options {
    Mode mode = PING;
    int domain = 0;
    std::vector<size_t> sizes;
    bool reliable = true;
    std::string transport = "udp";
    double warmup = 1, duration = 5, rate = 0, timeout = 60;
    long long count = -1;
    int depth = -1;
    double reply_timeout = 1.0;
    const char* label = nullptr;
    bool json = false;
    int participants = 2;
    double start = 0;  // seconds since the epoch, 0: this process's start
} o;

double now_s()
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec + (double)t.tv_nsec * 1e-9;
}

double wall_s()
{
    struct timespec t;
    clock_gettime(CLOCK_REALTIME, &t);
    return (double)t.tv_sec + (double)t.tv_nsec * 1e-9;
}

void sleep_s(double s)
{
    if (s > 0) {
        std::this_thread::sleep_for(std::chrono::duration<double>(s));
    }
}

[[noreturn]] void die(const char* what, DDS::ReturnCode_t rc = DDS::RETCODE_ERROR)
{
    std::fprintf(stderr, "%s: failed (%d)\n", what, (int)rc);
    std::exit(1);
}

[[noreturn]] void usage(const char* why)
{
    std::fprintf(stderr, "%s\nusage: perf ping|pong|pub|sub|disc [options] (see bench/opendds/perf.cpp)\n", why);
    std::exit(2);
}

void parse(int argc, char** argv)
{
    if (argc < 2) usage("no mode");
    std::string m = argv[1];
    if (m == "ping") o.mode = PING;
    else if (m == "pong") o.mode = PONG;
    else if (m == "pub") o.mode = PUB;
    else if (m == "sub") o.mode = SUB;
    else if (m == "disc") o.mode = DISC;
    else usage("unknown mode");
    for (int i = 2; i < argc; i++) {
        std::string a = argv[i];
        if (a == "--json") {
            o.json = true;
            continue;
        }
        if (i + 1 >= argc) usage("an option needs a value");
        const char* v = argv[++i];
        if (a == "--domain") o.domain = std::atoi(v);
        else if (a == "--size") o.sizes = {(size_t)std::atol(v)};
        else if (a == "--sizes") {
            o.sizes.clear();
            std::string s = v;
            size_t p = 0;
            while (p <= s.size() && o.sizes.size() < MAX_SIZES) {
                size_t q = s.find(',', p);
                if (q == std::string::npos) q = s.size();
                o.sizes.push_back((size_t)std::atol(s.substr(p, q - p).c_str()));
                p = q + 1;
            }
        }
        else if (a == "--reliability") o.reliable = std::string(v) == "reliable";
        else if (a == "--transport") o.transport = v;
        else if (a == "--warmup") o.warmup = std::atof(v);
        else if (a == "--duration") o.duration = std::atof(v);
        else if (a == "--count") o.count = std::atoll(v);
        else if (a == "--rate") o.rate = std::atof(v);
        else if (a == "--depth") o.depth = std::atoi(v);
        else if (a == "--reply-timeout") o.reply_timeout = std::atof(v) / 1000.0;
        else if (a == "--timeout") o.timeout = std::atof(v);
        else if (a == "--label") o.label = v;
        else if (a == "--participants") o.participants = std::atoi(v);
        else if (a == "--start") o.start = std::atof(v);
        else usage("unknown option");
    }
    if (o.transport != "udp") usage("OpenDDS has --transport udp only here");
    if (o.sizes.empty()) {
        if (o.mode == PING || o.mode == PONG)
            for (int e = 5; e <= 20; e++) o.sizes.push_back((size_t)1 << e);
        else
            o.sizes.push_back(1024);
    }
}

void json_head(size_t size)
{
    std::printf("{\"tool\":\"rustorb-dds-perf\",\"impl\":\"opendds\",\"version\":\"%s\","
                "\"mode\":\"%s\",\"size\":%zu,\"reliability\":\"%s\",\"transport\":\"%s\"",
                OPENDDS_VERSION, mode_name[o.mode], size, o.reliable ? "reliable" : "best-effort",
                o.transport.c_str());
    if (o.label) std::printf(",\"label\":\"%s\"", o.label);
}

// The nearest-rank percentile of sorted v.
double pct(const std::vector<double>& v, double p)
{
    if (v.empty()) return 0;
    size_t k = (size_t)std::ceil(p * (double)v.size() - 1e-9);
    k = std::max<size_t>(1, std::min(k, v.size()));
    return v[k - 1];
}

DDS::DomainParticipantFactory_var factory;
DDS::DomainParticipant_var participant;
RustorbPerf::SampleDataWriter_var writer;
RustorbPerf::SampleDataReader_var reader;
DDS::WaitSet_var waitset;
DDS::ReadCondition_var condition;

void set_representation(DDS::DataRepresentationIdSeq& seq, bool both)
{
    seq.length(both ? 2 : 1);
    seq[0] = DDS::XCDR_DATA_REPRESENTATION;
    if (both) seq[1] = DDS::XCDR2_DATA_REPRESENTATION;
}

void setup()
{
    bool throughput = o.mode == PUB || o.mode == SUB;
    int depth = o.depth > 0 ? o.depth : (throughput ? 100 : 1);
    const char *w = nullptr, *r = nullptr;
    switch (o.mode) {
        case PING: w = "RustorbPerf_ping"; r = "RustorbPerf_pong"; break;
        case PONG: w = "RustorbPerf_pong"; r = "RustorbPerf_ping"; break;
        case PUB: w = "RustorbPerf_data"; break;
        case SUB: r = "RustorbPerf_data"; break;
        case DISC: w = r = "RustorbPerf_disc"; break;
    }
    participant = factory->create_participant(o.domain, PARTICIPANT_QOS_DEFAULT, 0,
                                              OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    if (!participant) die("create_participant");
    RustorbPerf::SampleTypeSupport_var ts = new RustorbPerf::SampleTypeSupportImpl;
    if (ts->register_type(participant, "") != DDS::RETCODE_OK) die("register_type");
    CORBA::String_var type_name = ts->get_type_name();
    auto topic = [&](const char* name) {
        DDS::Topic_var t = participant->create_topic(name, type_name, TOPIC_QOS_DEFAULT, 0,
                                                     OpenDDS::DCPS::DEFAULT_STATUS_MASK);
        if (!t) die("create_topic");
        return t;
    };
    DDS::Topic_var wtopic;
    if (w) {
        wtopic = topic(w);
        DDS::Topic_var t = wtopic;
        DDS::Publisher_var pub =
            participant->create_publisher(PUBLISHER_QOS_DEFAULT, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
        DDS::DataWriterQos q;
        pub->get_default_datawriter_qos(q);
        if (o.reliable) {
            q.reliability.kind = DDS::RELIABLE_RELIABILITY_QOS;
            q.reliability.max_blocking_time.sec = 0;
            q.reliability.max_blocking_time.nanosec = 100000000;
        } else {
            q.reliability.kind = DDS::BEST_EFFORT_RELIABILITY_QOS;
        }
        if (throughput && o.reliable) {
            // Flow control: a write blocks once depth samples are unacknowledged.
            q.history.kind = DDS::KEEP_ALL_HISTORY_QOS;
            q.resource_limits.max_samples = depth;
            q.resource_limits.max_samples_per_instance = depth;
        } else {
            q.history.kind = DDS::KEEP_LAST_HISTORY_QOS;
            q.history.depth = depth;
        }
        set_representation(q.representation.value, false);
        DDS::DataWriter_var dw = pub->create_datawriter(t, q, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
        writer = RustorbPerf::SampleDataWriter::_narrow(dw);
        if (!writer) die("create_datawriter");
    }
    if (r) {
        DDS::Topic_var t = w && !std::strcmp(w, r) ? wtopic : topic(r);
        DDS::Subscriber_var sub =
            participant->create_subscriber(SUBSCRIBER_QOS_DEFAULT, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
        DDS::DataReaderQos q;
        sub->get_default_datareader_qos(q);
        q.reliability.kind = o.reliable ? DDS::RELIABLE_RELIABILITY_QOS : DDS::BEST_EFFORT_RELIABILITY_QOS;
        if (throughput) {
            q.history.kind = DDS::KEEP_ALL_HISTORY_QOS;
            // A best-effort sub slower than its pub drops what does not fit.
            if (!o.reliable) {
                q.resource_limits.max_samples = READER_MAX_SAMPLES;
                q.resource_limits.max_samples_per_instance = READER_MAX_SAMPLES;
            }
        } else {
            q.history.kind = DDS::KEEP_LAST_HISTORY_QOS;
            q.history.depth = depth;
        }
        set_representation(q.representation.value, true);
        DDS::DataReader_var dr = sub->create_datareader(t, q, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
        reader = RustorbPerf::SampleDataReader::_narrow(dr);
        if (!reader) die("create_datareader");
        condition = reader->create_readcondition(DDS::ANY_SAMPLE_STATE, DDS::ANY_VIEW_STATE,
                                                 DDS::ANY_INSTANCE_STATE);
        waitset = new DDS::WaitSet;
        waitset->attach_condition(condition);
    }
}

int writer_matched()
{
    DDS::PublicationMatchedStatus st = {};
    if (writer) writer->get_publication_matched_status(st);
    return st.current_count;
}

int reader_matched()
{
    DDS::SubscriptionMatchedStatus st = {};
    if (reader) reader->get_subscription_matched_status(st);
    return st.current_count;
}

void wait_for_match()
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

// Waits up to s seconds for data.
bool wait_data(double s)
{
    if (s <= 0) return false;
    DDS::ConditionSeq active;
    DDS::Duration_t d;
    d.sec = (CORBA::Long)s;
    d.nanosec = (CORBA::ULong)((s - (double)d.sec) * 1e9);
    return waitset->wait(active, d) == DDS::RETCODE_OK;
}

// Takes up to MAX_TAKE samples; returns how many.
int take(RustorbPerf::SampleSeq& data, DDS::SampleInfoSeq& infos)
{
    DDS::ReturnCode_t rc = reader->take(data, infos, MAX_TAKE, DDS::ANY_SAMPLE_STATE,
                                        DDS::ANY_VIEW_STATE, DDS::ANY_INSTANCE_STATE);
    if (rc == DDS::RETCODE_NO_DATA) return 0;
    if (rc != DDS::RETCODE_OK) die("take", rc);
    return (int)data.length();
}

RustorbPerf::Sample out_sample;

DDS::ReturnCode_t send(uint64_t seq, uint64_t stamp, size_t len)
{
    if (out_sample.payload.length() != len) {
        out_sample.payload.length((CORBA::ULong)len);
        if (len) std::memset(out_sample.payload.get_buffer(), 0x5a, len);
    }
    out_sample.seq = seq;
    out_sample.stamp = stamp;
    return writer->write(out_sample, DDS::HANDLE_NIL);
}

void ping()
{
    wait_for_match();
    double start = now_s();
    uint64_t seq = 0;
    RustorbPerf::SampleSeq data;
    DDS::SampleInfoSeq infos;
    if (!o.json)
        std::printf("%8s %8s %6s %9s %9s %9s %9s %9s  (round trip, us)\n", "size", "count", "lost", "p50",
                    "p90", "p99", "p99.9", "max");
    for (size_t size : o.sizes) {
        double warm_end = now_s() + o.warmup, end = warm_end + o.duration;
        std::vector<double> rtt;
        rtt.reserve(1 << 16);
        uint64_t lost = 0;
        if (take(data, infos) > 0) reader->return_loan(data, infos);
        for (;;) {
            double t = now_s();
            bool measuring = t >= warm_end;
            if (t >= end || (measuring && o.count >= 0 && (long long)rtt.size() >= o.count)) break;
            seq++;
            double sent = now_s();
            DDS::ReturnCode_t rc = send(seq, (uint64_t)((sent - start) * 1e9), size);
            if (rc == DDS::RETCODE_TIMEOUT) {
                if (measuring) lost++;
                continue;
            }
            if (rc != DDS::RETCODE_OK) die("write", rc);
            double deadline = sent + o.reply_timeout;
            for (;;) {
                double left = deadline - now_s();
                if (left <= 0 || !wait_data(left)) {
                    if (measuring) lost++;
                    break;
                }
                int k = take(data, infos);
                bool found = false;
                for (int j = 0; j < k; j++)
                    if (infos[j].valid_data && data[j].seq == seq) found = true;
                if (k > 0) reader->return_loan(data, infos);
                if (found) {
                    if (measuring) rtt.push_back((now_s() - sent) * 1e6);
                    break;
                }
            }
        }
        size_t n = rtt.size();
        double mean = 0;
        for (double x : rtt) mean += x;
        mean = n ? mean / (double)n : 0;
        std::sort(rtt.begin(), rtt.end());
        double mn = n ? rtt.front() : 0, mx = n ? rtt.back() : 0;
        if (o.json) {
            json_head(size);
            std::printf(",\"count\":%zu,\"lost\":%llu,\"seconds\":%.3f,\"rtt_min_us\":%.3f,\"rtt_mean_us\":%.3f,"
                        "\"rtt_p50_us\":%.3f,\"rtt_p90_us\":%.3f,\"rtt_p99_us\":%.3f,\"rtt_p999_us\":%.3f,"
                        "\"rtt_max_us\":%.3f}\n",
                        n, (unsigned long long)lost, o.duration, mn, mean, pct(rtt, 0.5), pct(rtt, 0.9),
                        pct(rtt, 0.99), pct(rtt, 0.999), mx);
        } else {
            std::printf("%8zu %8zu %6llu %9.1f %9.1f %9.1f %9.1f %9.1f\n", size, n, (unsigned long long)lost,
                        pct(rtt, 0.5), pct(rtt, 0.9), pct(rtt, 0.99), pct(rtt, 0.999), mx);
        }
        std::fflush(stdout);
    }
}

void pong()
{
    double deadline = now_s() + o.timeout;
    bool matched = false;
    uint64_t echoed = 0;
    RustorbPerf::SampleSeq data;
    DDS::SampleInfoSeq infos;
    for (;;) {
        wait_data(0.2);
        int k = take(data, infos);
        for (int j = 0; j < k; j++) {
            if (!infos[j].valid_data) continue;
            DDS::ReturnCode_t rc = writer->write(data[j], DDS::HANDLE_NIL);
            if (rc != DDS::RETCODE_OK && rc != DDS::RETCODE_TIMEOUT) die("write", rc);
            echoed++;
        }
        if (k > 0) reader->return_loan(data, infos);
        int w = writer_matched(), r = reader_matched();
        if (matched) {
            if (w == 0 && r == 0) break;
        } else if (w || r) {
            matched = true;
        } else if (now_s() > deadline) {
            std::fprintf(stderr, "pong: no ping in %.0f s\n", o.timeout);
            std::exit(1);
        }
    }
    if (!o.json) std::printf("pong: sent back %llu samples\n", (unsigned long long)echoed);
}

void publish()
{
    wait_for_match();
    size_t size = o.sizes[0];
    double start = now_s(), give_up = start + o.warmup + o.duration + o.timeout;
    uint64_t seq = 0, blocked = 0;
    for (;;) {
        if (seq % 256 == 0 && (writer_matched() == 0 || now_s() >= give_up)) break;
        DDS::ReturnCode_t rc = send(seq + 1, (uint64_t)((now_s() - start) * 1e9), size);
        if (rc == DDS::RETCODE_OK) {
            seq++;
        } else if (rc == DDS::RETCODE_TIMEOUT) {
            blocked++;
            if (writer_matched() == 0) break;
        } else {
            die("write", rc);
        }
        if (o.rate > 0) sleep_s(start + (double)seq / o.rate - now_s());
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

void subscribe()
{
    wait_for_match();
    size_t size = o.sizes[0];
    if (!wait_data(o.timeout)) {
        std::fprintf(stderr, "sub: no sample in %.0f s\n", o.timeout);
        std::exit(1);
    }
    double warm_end = now_s() + o.warmup, end = warm_end + o.duration;
    double second_end = warm_end + 1;
    uint64_t samples_n = 0, octets = 0, first = 0, last = 0, in_second = 0;
    uint64_t min_s = UINT64_MAX, max_s = 0;
    bool have_first = false;
    int seconds = 0;
    RustorbPerf::SampleSeq data;
    DDS::SampleInfoSeq infos;
    for (;;) {
        double t = now_s();
        if (t >= end) break;
        if (t >= second_end && t > warm_end) {
            min_s = std::min(min_s, in_second);
            max_s = std::max(max_s, in_second);
            seconds++;
            in_second = 0;
            second_end += 1;
        }
        double left = end - t;
        wait_data(left < 0.05 ? left : 0.05);
        for (;;) {
            int k = take(data, infos);
            if (k <= 0) break;
            bool counting = now_s() >= warm_end;
            for (int j = 0; j < k && counting; j++) {
                if (!infos[j].valid_data) continue;
                samples_n++;
                in_second++;
                octets += data[j].payload.length();
                if (!have_first) {
                    first = data[j].seq;
                    have_first = true;
                }
                last = std::max<uint64_t>(last, data[j].seq);
            }
            reader->return_loan(data, infos);
            if (k < MAX_TAKE) break;
        }
    }
    if (!seconds) min_s = 0;
    uint64_t lost = 0;
    if (have_first && last - first + 1 > samples_n) lost = last - first + 1 - samples_n;
    double secs = o.duration, mbit = (double)octets * 8 / secs / 1e6;
    if (o.json) {
        json_head(size);
        std::printf(",\"samples\":%llu,\"octets\":%llu,\"lost\":%llu,\"seconds\":%.3f,\"samples_per_s\":%.3f,"
                    "\"mbit_per_s\":%.3f,\"samples_per_s_min\":%llu,\"samples_per_s_max\":%llu}\n",
                    (unsigned long long)samples_n, (unsigned long long)octets, (unsigned long long)lost, secs,
                    (double)samples_n / secs, mbit, (unsigned long long)min_s, (unsigned long long)max_s);
    } else {
        std::printf("sub: %llu samples of %zu octets in %.1f s: %.0f/s, %.1f Mbit/s, %llu lost\n",
                    (unsigned long long)samples_n, size, secs, (double)samples_n / secs, mbit,
                    (unsigned long long)lost);
    }
}

// disc: the seconds from the common start until the participant is
// created and until its writer and reader match those of all the
// participants (its own included); stays to --duration for the others.
void discovery()
{
    double start = o.start > 0 ? o.start : wall_s();
    setup();
    double created = wall_s() - start, matched = -1;
    while (matched < 0 && wall_s() < start + o.duration) {
        if (writer_matched() >= o.participants && reader_matched() >= o.participants)
            matched = wall_s() - start;
        else
            sleep_s(0.001);
    }
    sleep_s(start + o.duration - wall_s());
    if (o.json) {
        std::printf("{\"tool\":\"rustorb-dds-perf\",\"impl\":\"opendds\",\"version\":\"%s\",\"mode\":\"disc\","
                    "\"transport\":\"%s\",\"participants\":%d,\"create_s\":%.6f,\"writer_matched\":%d,"
                    "\"reader_matched\":%d",
                    OPENDDS_VERSION, o.transport.c_str(), o.participants, created, writer_matched(),
                    reader_matched());
        if (matched >= 0) std::printf(",\"match_s\":%.6f", matched);
        if (o.label) std::printf(",\"label\":\"%s\"", o.label);
        std::printf("}\n");
    } else if (matched >= 0) {
        std::printf("disc: participant at %.3f s, all matched at %.3f s\n", created, matched);
    } else {
        std::printf("disc: matched %d writers and %d readers of %d\n", writer_matched(), reader_matched(),
                    o.participants);
    }
}

}  // namespace

int main(int argc, char** argv)
{
    // Takes the -DCPS* arguments (the configuration file) out of argv.
    factory = TheParticipantFactoryWithArgs(argc, argv);
    if (!factory) die("participant factory");
    parse(argc, argv);
    setvbuf(stdout, nullptr, _IOLBF, 0);
    if (o.mode == DISC) {
        discovery();
    } else {
        setup();
        switch (o.mode) {
            case PING: ping(); break;
            case PONG: pong(); break;
            case PUB: publish(); break;
            case SUB: subscribe(); break;
            case DISC: break;
        }
    }
    if (waitset && condition) waitset->detach_condition(condition);
    participant->delete_contained_entities();
    factory->delete_participant(participant);
    TheServiceParticipant->shutdown();
    return 0;
}

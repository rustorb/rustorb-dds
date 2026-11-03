// Hello world publisher and subscriber for Fast DDS, written against its
// public C++ API only. Used by interop/run.sh.
//
//   hello pub <domain> <count> <interval_ms> [best_effort|reliable|transient_local [size]]
//   hello sub <domain> <count> <timeout_s> [best_effort|reliable|transient_local [size]]
//
// The publisher waits for a matched reader, then writes <count> samples.
// The subscriber exits with 0 after <count> valid samples, with 1 on
// timeout.
//
// Best effort by default. When reliable, both sides keep every sample
// (KEEP_ALL), the publisher waits for acknowledgments before it exits and
// the subscriber checks that it gets 1, 2, 3... with none missing.
//
// transient_local is reliable and TRANSIENT_LOCAL: the publisher writes at
// once, before any subscriber is there, then waits for one and for its
// acknowledgments; the subscriber creates its reader two seconds late.
//
// The message is "Hello, world!", or with <size>, <size> letters that
// depend on the index ('a' + (i + index) % 26 for the i-th), so that the
// sample is too large for a datagram.

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <thread>

#include <fastdds/dds/domain/DomainParticipant.hpp>
#include <fastdds/dds/domain/DomainParticipantFactory.hpp>
#include <fastdds/dds/publisher/DataWriter.hpp>
#include <fastdds/dds/publisher/Publisher.hpp>
#include <fastdds/dds/subscriber/DataReader.hpp>
#include <fastdds/dds/subscriber/SampleInfo.hpp>
#include <fastdds/dds/subscriber/Subscriber.hpp>
#include <fastdds/dds/topic/TypeSupport.hpp>

#include "HelloWorldPubSubTypes.h"

using namespace eprosima::fastdds::dds;

static const char* MESSAGE = "Hello, world!";
static bool reliable = false;
static bool transient_local = false;

static void wait_for_reader(DataWriter* writer)
{
    for (int i = 0; i < 300; i++) {
        PublicationMatchedStatus st;
        writer->get_publication_matched_status(st);
        if (st.current_count > 0) {
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
}
static long size = -1;

static std::string message(uint32_t index)
{
    if (size < 0) {
        return MESSAGE;
    }
    std::string m(static_cast<size_t>(size), 'a');
    for (long i = 0; i < size; i++) {
        m[static_cast<size_t>(i)] = static_cast<char>('a' + (i + index) % 26);
    }
    return m;
}

static int publish(DomainParticipant* pp, Topic* topic, int count, int interval_ms)
{
    Publisher* pub = pp->create_publisher(PUBLISHER_QOS_DEFAULT);
    DataWriterQos qos = DATAWRITER_QOS_DEFAULT;
    if (reliable) {
        qos.reliability().kind = RELIABLE_RELIABILITY_QOS;
        qos.history().kind = KEEP_ALL_HISTORY_QOS;
    } else {
        qos.reliability().kind = BEST_EFFORT_RELIABILITY_QOS;
    }
    qos.durability().kind = transient_local ? TRANSIENT_LOCAL_DURABILITY_QOS : VOLATILE_DURABILITY_QOS;
    DataWriter* writer = pub->create_datawriter(topic, qos);
    if (writer == nullptr) {
        std::fprintf(stderr, "fastdds: create_datawriter failed\n");
        return 1;
    }
    if (!transient_local) {
        wait_for_reader(writer);
        // Give the reader time to match this writer too.
        std::this_thread::sleep_for(std::chrono::milliseconds(500));
    }
    for (int i = 1; i <= count; i++) {
        HelloWorld::Greeting g;
        g.index(static_cast<uint32_t>(i));
        g.message(message(static_cast<uint32_t>(i)));
        if (!writer->write(&g)) {
            std::fprintf(stderr, "fastdds: write failed\n");
            return 1;
        }
        std::printf("fastdds: sent %d\n", i);
        std::fflush(stdout);
        std::this_thread::sleep_for(std::chrono::milliseconds(interval_ms));
    }
    if (transient_local) {
        wait_for_reader(writer);
    }
    if (reliable) {
        if (writer->wait_for_acknowledgments(eprosima::fastrtps::Duration_t(30, 0)) !=
            ReturnCode_t::RETCODE_OK) {
            std::fprintf(stderr, "fastdds: wait_for_acknowledgments failed\n");
            return 1;
        }
        std::printf("fastdds: all acknowledged\n");
    }
    return 0;
}

static int subscribe(DomainParticipant* pp, Topic* topic, int count, int timeout_s)
{
    Subscriber* sub = pp->create_subscriber(SUBSCRIBER_QOS_DEFAULT);
    DataReaderQos qos = DATAREADER_QOS_DEFAULT;
    qos.reliability().kind = reliable ? RELIABLE_RELIABILITY_QOS : BEST_EFFORT_RELIABILITY_QOS;
    qos.history().kind = KEEP_ALL_HISTORY_QOS;
    qos.durability().kind = transient_local ? TRANSIENT_LOCAL_DURABILITY_QOS : VOLATILE_DURABILITY_QOS;
    if (transient_local) {
        std::this_thread::sleep_for(std::chrono::seconds(2)); // join late
    }
    DataReader* reader = sub->create_datareader(topic, qos);
    if (reader == nullptr) {
        std::fprintf(stderr, "fastdds: create_datareader failed\n");
        return 1;
    }
    int got = 0;
    auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(timeout_s);
    while (got < count && std::chrono::steady_clock::now() < deadline) {
        HelloWorld::Greeting g;
        SampleInfo info;
        if (reader->take_next_sample(&g, &info) == ReturnCode_t::RETCODE_OK) {
            if (!info.valid_data) {
                continue;
            }
            if (g.message() != message(g.index())) {
                std::fprintf(stderr, "fastdds: bad message \"%.40s\" (%zu octets) for %u\n",
                             g.message().c_str(), g.message().size(), g.index());
                return 1;
            }
            std::printf("fastdds: received %u\n", g.index());
            std::fflush(stdout);
            got++;
            if (reliable && g.index() != static_cast<uint32_t>(got)) {
                std::fprintf(stderr, "fastdds: expected %d, got %u\n", got, g.index());
                return 1;
            }
        } else {
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        }
    }
    if (got < count) {
        std::fprintf(stderr, "fastdds: got %d of %d samples\n", got, count);
        return 1;
    }
    return 0;
}

int main(int argc, char** argv)
{
    if (argc < 5 || argc > 7) {
        std::fprintf(stderr,
                     "usage: %s pub|sub <domain> <count> <interval_ms|timeout_s> [best_effort|reliable|transient_local [size]]\n",
                     argv[0]);
        return 2;
    }
    transient_local = argc >= 6 && std::string(argv[5]) == "transient_local";
    reliable = transient_local || (argc >= 6 && std::string(argv[5]) == "reliable");
    if (argc == 7) {
        size = std::atol(argv[6]);
    }
    int domain = std::atoi(argv[2]);
    int count = std::atoi(argv[3]);
    int arg = std::atoi(argv[4]);
    DomainParticipant* pp =
        DomainParticipantFactory::get_instance()->create_participant(domain, PARTICIPANT_QOS_DEFAULT);
    if (pp == nullptr) {
        std::fprintf(stderr, "fastdds: create_participant failed\n");
        return 1;
    }
    TypeSupport type(new HelloWorld::GreetingPubSubType());
    type.register_type(pp);
    Topic* topic = pp->create_topic("hello_world", type.get_type_name(), TOPIC_QOS_DEFAULT);
    int rc = std::string(argv[1]) == "pub" ? publish(pp, topic, count, arg)
                                           : subscribe(pp, topic, count, arg);
    pp->delete_contained_entities();
    DomainParticipantFactory::get_instance()->delete_participant(pp);
    return rc;
}

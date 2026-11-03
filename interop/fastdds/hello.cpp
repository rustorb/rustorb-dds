// Hello world publisher and subscriber for Fast DDS, written against its
// public C++ API only. Used by interop/run.sh.
//
//   hello pub <domain> <count> <interval_ms>
//   hello sub <domain> <count> <timeout_s>
//
// The publisher waits for a matched reader, then writes <count> samples.
// The subscriber exits with 0 after <count> valid samples, with 1 on
// timeout.

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

static int publish(DomainParticipant* pp, Topic* topic, int count, int interval_ms)
{
    Publisher* pub = pp->create_publisher(PUBLISHER_QOS_DEFAULT);
    DataWriterQos qos = DATAWRITER_QOS_DEFAULT;
    qos.reliability().kind = BEST_EFFORT_RELIABILITY_QOS;
    DataWriter* writer = pub->create_datawriter(topic, qos);
    if (writer == nullptr) {
        std::fprintf(stderr, "fastdds: create_datawriter failed\n");
        return 1;
    }
    for (int i = 0; i < 300; i++) {
        PublicationMatchedStatus st;
        writer->get_publication_matched_status(st);
        if (st.current_count > 0) {
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
    // Give the reader time to match this writer too.
    std::this_thread::sleep_for(std::chrono::milliseconds(500));
    for (int i = 1; i <= count; i++) {
        HelloWorld::Greeting g;
        g.index(static_cast<uint32_t>(i));
        g.message(MESSAGE);
        if (!writer->write(&g)) {
            std::fprintf(stderr, "fastdds: write failed\n");
            return 1;
        }
        std::printf("fastdds: sent %d\n", i);
        std::fflush(stdout);
        std::this_thread::sleep_for(std::chrono::milliseconds(interval_ms));
    }
    return 0;
}

static int subscribe(DomainParticipant* pp, Topic* topic, int count, int timeout_s)
{
    Subscriber* sub = pp->create_subscriber(SUBSCRIBER_QOS_DEFAULT);
    DataReaderQos qos = DATAREADER_QOS_DEFAULT;
    qos.reliability().kind = BEST_EFFORT_RELIABILITY_QOS;
    qos.history().kind = KEEP_ALL_HISTORY_QOS;
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
            if (g.message() != MESSAGE) {
                std::fprintf(stderr, "fastdds: bad message \"%s\"\n", g.message().c_str());
                return 1;
            }
            std::printf("fastdds: received %u\n", g.index());
            std::fflush(stdout);
            got++;
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
    if (argc != 5) {
        std::fprintf(stderr, "usage: %s pub|sub <domain> <count> <interval_ms|timeout_s>\n", argv[0]);
        return 2;
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

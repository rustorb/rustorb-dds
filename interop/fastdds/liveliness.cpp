// LIVELINESS across participants, for interop/run.sh: the same as the
// liveliness example of rustorb-dds (see its documentation). Written
// against Fast DDS's public C++ API only.
//
//   liveliness pub <domain> <automatic|participant|topic> <lease_ms>
//   liveliness sub <domain> <automatic|participant|topic> <lease_ms> <timeout_s>

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <thread>

#include <fastdds/dds/core/status/LivelinessChangedStatus.hpp>
#include <fastdds/dds/core/status/BaseStatus.hpp>
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
using Clock = std::chrono::steady_clock;

static LivelinessQosPolicyKind kind;
static std::chrono::milliseconds lease;

static eprosima::fastrtps::Duration_t duration(std::chrono::milliseconds d)
{
    return eprosima::fastrtps::Duration_t(static_cast<double>(d.count()) / 1000.0);
}

static void set_liveliness(LivelinessQosPolicy& l)
{
    l.kind = kind;
    l.lease_duration = duration(lease);
    // Fast DDS asserts AUTOMATIC writers at this period; it must be below
    // the lease.
    l.announcement_period = duration(lease / 3);
}

static bool write_one(DataWriter* writer, uint32_t index)
{
    HelloWorld::Greeting g;
    g.index(index);
    g.message("Hello, world!");
    if (!writer->write(&g)) {
        std::fprintf(stderr, "fastdds: write failed\n");
        return false;
    }
    std::printf("fastdds: sent %u\n", index);
    std::fflush(stdout);
    return true;
}

static int publish(DomainParticipant* pp, Topic* topic)
{
    Publisher* pub = pp->create_publisher(PUBLISHER_QOS_DEFAULT);
    DataWriterQos qos = DATAWRITER_QOS_DEFAULT;
    qos.reliability().kind = RELIABLE_RELIABILITY_QOS;
    qos.history().kind = KEEP_ALL_HISTORY_QOS;
    set_liveliness(qos.liveliness());
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
    std::this_thread::sleep_for(std::chrono::milliseconds(500));
    if (!write_one(writer, 1)) {
        return 1;
    }
    auto until = Clock::now() + lease * 3;
    while (Clock::now() < until) {
        if (kind == MANUAL_BY_PARTICIPANT_LIVELINESS_QOS) {
            pp->assert_liveliness();
        } else if (kind == MANUAL_BY_TOPIC_LIVELINESS_QOS) {
            writer->assert_liveliness();
        }
        std::this_thread::sleep_for(lease / 4);
    }
    if (!write_one(writer, 2)) {
        return 1;
    }
    std::this_thread::sleep_for(lease * 4);
    if (!write_one(writer, 3)) {
        return 1;
    }
    if (writer->wait_for_acknowledgments(eprosima::fastrtps::Duration_t(30, 0)) !=
        ReturnCode_t::RETCODE_OK) {
        std::fprintf(stderr, "fastdds: wait_for_acknowledgments failed\n");
        return 1;
    }
    std::this_thread::sleep_for(std::chrono::seconds(1));
    LivelinessLostStatus lost;
    writer->get_liveliness_lost_status(lost);
    std::printf("fastdds: liveliness lost %d times\n", lost.total_count);
    return 0;
}

static int subscribe(DomainParticipant* pp, Topic* topic, int timeout_s)
{
    Subscriber* sub = pp->create_subscriber(SUBSCRIBER_QOS_DEFAULT);
    DataReaderQos qos = DATAREADER_QOS_DEFAULT;
    qos.reliability().kind = RELIABLE_RELIABILITY_QOS;
    qos.history().kind = KEEP_ALL_HISTORY_QOS;
    set_liveliness(qos.liveliness());
    DataReader* reader = sub->create_datareader(topic, qos);
    if (reader == nullptr) {
        std::fprintf(stderr, "fastdds: create_datareader failed\n");
        return 1;
    }
    auto deadline = Clock::now() + std::chrono::seconds(timeout_s);
    uint32_t last = 0;
    bool not_alive_seen = false;
    while (Clock::now() < deadline) {
        LivelinessChangedStatus s;
        reader->get_liveliness_changed_status(s);
        if (last >= 1 && s.not_alive_count > 0 && !not_alive_seen) {
            not_alive_seen = true;
            std::printf("fastdds: writer not alive after sample %u\n", last);
            if (last != 2 || kind == AUTOMATIC_LIVELINESS_QOS) {
                std::fprintf(stderr, "fastdds: writer not alive after sample %u\n", last);
                return 1;
            }
        }
        if (last == 3) {
            if (s.alive_count == 1 && s.not_alive_count == 0) {
                std::printf("fastdds: writer alive again\n");
                return 0;
            }
        } else {
            HelloWorld::Greeting g;
            SampleInfo info;
            while (reader->take_next_sample(&g, &info) == ReturnCode_t::RETCODE_OK) {
                if (!info.valid_data) {
                    continue;
                }
                if (g.index() != last + 1) {
                    std::fprintf(stderr, "fastdds: expected %u, got %u\n", last + 1, g.index());
                    return 1;
                }
                last = g.index();
                std::printf("fastdds: received %u\n", last);
                std::fflush(stdout);
                if (last == 3 && kind != AUTOMATIC_LIVELINESS_QOS && !not_alive_seen) {
                    std::fprintf(stderr, "fastdds: the writer was never seen not alive\n");
                    return 1;
                }
            }
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    std::fprintf(stderr, "fastdds: timed out after sample %u\n", last);
    return 1;
}

int main(int argc, char** argv)
{
    if (argc < 5) {
        std::fprintf(stderr,
                     "usage: %s pub|sub <domain> <automatic|participant|topic> <lease_ms> [timeout_s]\n",
                     argv[0]);
        return 2;
    }
    std::string k = argv[3];
    if (k == "automatic") {
        kind = AUTOMATIC_LIVELINESS_QOS;
    } else if (k == "participant") {
        kind = MANUAL_BY_PARTICIPANT_LIVELINESS_QOS;
    } else if (k == "topic") {
        kind = MANUAL_BY_TOPIC_LIVELINESS_QOS;
    } else {
        std::fprintf(stderr, "unknown liveliness kind %s\n", argv[3]);
        return 2;
    }
    lease = std::chrono::milliseconds(std::atoi(argv[4]));
    DomainParticipant* pp = DomainParticipantFactory::get_instance()->create_participant(
        std::atoi(argv[2]), PARTICIPANT_QOS_DEFAULT);
    if (pp == nullptr) {
        std::fprintf(stderr, "fastdds: create_participant failed\n");
        return 1;
    }
    TypeSupport type(new HelloWorld::GreetingPubSubType());
    type.register_type(pp);
    Topic* topic = pp->create_topic("liveliness", type.get_type_name(), TOPIC_QOS_DEFAULT);
    int rc = std::string(argv[1]) == "pub" ? publish(pp, topic)
                                           : subscribe(pp, topic, argc > 5 ? std::atoi(argv[5]) : 60);
    pp->delete_contained_entities();
    DomainParticipantFactory::get_instance()->delete_participant(pp);
    return rc;
}

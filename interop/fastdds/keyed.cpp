// Keyed instances for Fast DDS, written against its public C++ API only.
// Used by interop/run.sh (rustorb-dds#17).
//
//   keyed pub <domain> <count> <interval_ms>
//   keyed sub <domain> <count> <timeout_s>
//
// Both sides are RELIABLE and KEEP_ALL. The publisher waits for a matched
// reader, writes <count> samples to the instances id 1, 2, 3 in turn
// (index 1, 2, 3...), then disposes id 2 and unregisters id 3 (the writer
// does not dispose unregistered instances), and waits for
// acknowledgments.
//
// The subscriber exits with 0 once it has <count> valid samples, each
// instance's in order, and has seen id 2 NOT_ALIVE_DISPOSED and id 3
// NOT_ALIVE_NO_WRITERS; with 1 on timeout or a wrong sample.

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

#include "KeyedPubSubTypes.h"

using namespace eprosima::fastdds::dds;

static const char* MESSAGE = "Hello, instance!";

static int publish(DomainParticipant* pp, Topic* topic, int count, int interval_ms)
{
    Publisher* pub = pp->create_publisher(PUBLISHER_QOS_DEFAULT);
    DataWriterQos qos = DATAWRITER_QOS_DEFAULT;
    qos.reliability().kind = RELIABLE_RELIABILITY_QOS;
    qos.history().kind = KEEP_ALL_HISTORY_QOS;
    qos.writer_data_lifecycle().autodispose_unregistered_instances = false;
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
    for (int i = 1; i <= count; i++) {
        Keyed::Shape s;
        s.id(static_cast<uint32_t>((i - 1) % 3 + 1));
        s.index(static_cast<uint32_t>(i));
        s.message(MESSAGE);
        if (!writer->write(&s)) {
            std::fprintf(stderr, "fastdds: write failed\n");
            return 1;
        }
        std::printf("fastdds: sent id %u index %d\n", s.id(), i);
        std::fflush(stdout);
        std::this_thread::sleep_for(std::chrono::milliseconds(interval_ms));
    }
    Keyed::Shape two;
    two.id(2);
    Keyed::Shape three;
    three.id(3);
    if (writer->dispose(&two, HANDLE_NIL) != ReturnCode_t::RETCODE_OK ||
        writer->unregister_instance(&three, HANDLE_NIL) != ReturnCode_t::RETCODE_OK) {
        std::fprintf(stderr, "fastdds: dispose or unregister failed\n");
        return 1;
    }
    std::printf("fastdds: disposed 2, unregistered 3\n");
    std::fflush(stdout);
    if (writer->wait_for_acknowledgments(eprosima::fastrtps::Duration_t(30, 0)) !=
        ReturnCode_t::RETCODE_OK) {
        std::fprintf(stderr, "fastdds: wait_for_acknowledgments failed\n");
        return 1;
    }
    std::printf("fastdds: all acknowledged\n");
    return 0;
}

static const char* state_name(InstanceStateKind s)
{
    switch (s) {
        case ALIVE_INSTANCE_STATE: return "ALIVE";
        case NOT_ALIVE_DISPOSED_INSTANCE_STATE: return "NOT_ALIVE_DISPOSED";
        case NOT_ALIVE_NO_WRITERS_INSTANCE_STATE: return "NOT_ALIVE_NO_WRITERS";
        default: return "?";
    }
}

static int subscribe(DomainParticipant* pp, Topic* topic, int count, int timeout_s)
{
    Subscriber* sub = pp->create_subscriber(SUBSCRIBER_QOS_DEFAULT);
    DataReaderQos qos = DATAREADER_QOS_DEFAULT;
    qos.reliability().kind = RELIABLE_RELIABILITY_QOS;
    qos.history().kind = KEEP_ALL_HISTORY_QOS;
    DataReader* reader = sub->create_datareader(topic, qos);
    if (reader == nullptr) {
        std::fprintf(stderr, "fastdds: create_datareader failed\n");
        return 1;
    }
    int got = 0;
    bool disposed = false, no_writers = false;
    uint32_t last[4] = {0, 0, 0, 0};
    // Invalid samples carry no key here: remember each instance's id.
    InstanceHandle_t handles[4];
    auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(timeout_s);
    while (!(got >= count && disposed && no_writers) && std::chrono::steady_clock::now() < deadline) {
        Keyed::Shape s;
        SampleInfo info;
        if (reader->take_next_sample(&s, &info) != ReturnCode_t::RETCODE_OK) {
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
            continue;
        }
        if (info.valid_data) {
            uint32_t id = s.id();
            if (id < 1 || id > 3 || s.message() != MESSAGE || s.index() <= last[id] ||
                (s.index() - 1) % 3 + 1 != id) {
                std::fprintf(stderr, "fastdds: bad sample id %u index %u\n", id, s.index());
                return 1;
            }
            handles[id] = info.instance_handle;
            last[id] = s.index();
            got++;
            std::printf("fastdds: received id %u index %u\n", id, s.index());
        } else {
            uint32_t id = 0;
            for (uint32_t i = 1; i <= 3; i++) {
                if (handles[i] == info.instance_handle) {
                    id = i;
                }
            }
            std::printf("fastdds: id %u is %s\n", id, state_name(info.instance_state));
            if (id == 2 && info.instance_state == NOT_ALIVE_DISPOSED_INSTANCE_STATE) {
                disposed = true;
            }
            if (id == 3 && info.instance_state == NOT_ALIVE_NO_WRITERS_INSTANCE_STATE) {
                no_writers = true;
            }
        }
        std::fflush(stdout);
    }
    if (got < count || !disposed || !no_writers) {
        std::fprintf(stderr, "fastdds: got %d of %d samples, disposed %d, no writers %d\n", got, count,
                     disposed, no_writers);
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
    DomainParticipant* pp = DomainParticipantFactory::get_instance()->create_participant(
        std::atoi(argv[2]), PARTICIPANT_QOS_DEFAULT);
    if (pp == nullptr) {
        std::fprintf(stderr, "fastdds: create_participant failed\n");
        return 1;
    }
    TypeSupport type(new Keyed::ShapePubSubType());
    type.register_type(pp);
    Topic* topic = pp->create_topic("keyed", type.get_type_name(), TOPIC_QOS_DEFAULT);
    int count = std::atoi(argv[3]), arg = std::atoi(argv[4]);
    int rc = std::string(argv[1]) == "pub" ? publish(pp, topic, count, arg)
                                           : subscribe(pp, topic, count, arg);
    pp->delete_contained_entities();
    DomainParticipantFactory::get_instance()->delete_participant(pp);
    return rc;
}

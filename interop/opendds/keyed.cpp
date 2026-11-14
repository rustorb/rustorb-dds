// Keyed instances for OpenDDS, written against its public API only. Used
// by interop/run.sh; the same as interop/fastdds/keyed.cpp.
//
//   keyed pub <domain> <count> <interval_ms>
//   keyed sub <domain> <count> <timeout_s>
//
// Both sides are RELIABLE and KEEP_ALL. The publisher waits for a matched
// reader, writes <count> samples to the instances id 1, 2, 3 in turn
// (index 1, 2, 3...), then disposes id 2 and unregisters id 3 (the writer
// does not dispose unregistered instances), and waits for
// acknowledgments. The subscriber exits with 0 once it has <count> valid
// samples, each instance's in order, and has seen id 2
// NOT_ALIVE_DISPOSED and id 3 NOT_ALIVE_NO_WRITERS; with 1 on timeout or
// a wrong sample.

#include "common.h"

#include "KeyedTypeSupportImpl.h"

using namespace interop;

static const char* MESSAGE = "Hello, instance!";

static int publish(DDS::DomainParticipant_ptr pp, DDS::Topic_ptr topic, int count, int interval_ms)
{
    DDS::Publisher_var pub =
        pp->create_publisher(PUBLISHER_QOS_DEFAULT, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    DDS::DataWriterQos qos;
    pub->get_default_datawriter_qos(qos);
    qos.reliability.kind = DDS::RELIABLE_RELIABILITY_QOS;
    qos.history.kind = DDS::KEEP_ALL_HISTORY_QOS;
    qos.writer_data_lifecycle.autodispose_unregistered_instances = false;
    set_representation(qos.representation.value, true, false);
    DDS::DataWriter_var w =
        pub->create_datawriter(topic, qos, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    Keyed::ShapeDataWriter_var writer = Keyed::ShapeDataWriter::_narrow(w);
    if (!writer) {
        std::fprintf(stderr, "opendds: create_datawriter failed\n");
        return 1;
    }
    wait_for_reader(writer);
    sleep_ms(500);
    for (int i = 1; i <= count; i++) {
        Keyed::Shape s;
        s.id = static_cast<CORBA::ULong>((i - 1) % 3 + 1);
        s.index = static_cast<CORBA::ULong>(i);
        s.message = MESSAGE;
        if (writer->write(s, DDS::HANDLE_NIL) != DDS::RETCODE_OK) {
            std::fprintf(stderr, "opendds: write failed\n");
            return 1;
        }
        std::printf("opendds: sent id %u index %d\n", static_cast<unsigned>(s.id), i);
        std::fflush(stdout);
        sleep_ms(interval_ms);
    }
    Keyed::Shape two;
    two.id = 2;
    two.index = 0;
    two.message = "";
    Keyed::Shape three;
    three.id = 3;
    three.index = 0;
    three.message = "";
    if (writer->dispose(two, DDS::HANDLE_NIL) != DDS::RETCODE_OK ||
        writer->unregister_instance(three, DDS::HANDLE_NIL) != DDS::RETCODE_OK) {
        std::fprintf(stderr, "opendds: dispose or unregister failed\n");
        return 1;
    }
    std::printf("opendds: disposed 2, unregistered 3\n");
    std::fflush(stdout);
    return wait_for_acknowledgments(writer) ? 0 : 1;
}

static const char* state_name(DDS::InstanceStateKind s)
{
    switch (s) {
        case DDS::ALIVE_INSTANCE_STATE: return "ALIVE";
        case DDS::NOT_ALIVE_DISPOSED_INSTANCE_STATE: return "NOT_ALIVE_DISPOSED";
        case DDS::NOT_ALIVE_NO_WRITERS_INSTANCE_STATE: return "NOT_ALIVE_NO_WRITERS";
        default: return "?";
    }
}

static int subscribe(DDS::DomainParticipant_ptr pp, DDS::Topic_ptr topic, int count, int timeout_s)
{
    DDS::Subscriber_var sub =
        pp->create_subscriber(SUBSCRIBER_QOS_DEFAULT, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    DDS::DataReaderQos qos;
    sub->get_default_datareader_qos(qos);
    qos.reliability.kind = DDS::RELIABLE_RELIABILITY_QOS;
    qos.history.kind = DDS::KEEP_ALL_HISTORY_QOS;
    set_representation(qos.representation.value, true, true);
    DDS::DataReader_var r =
        sub->create_datareader(topic, qos, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    Keyed::ShapeDataReader_var reader = Keyed::ShapeDataReader::_narrow(r);
    if (!reader) {
        std::fprintf(stderr, "opendds: create_datareader failed\n");
        return 1;
    }
    int got = 0;
    bool disposed = false, no_writers = false;
    CORBA::ULong last[4] = {0, 0, 0, 0};
    // Remember each instance's id, for the samples without data.
    DDS::InstanceHandle_t handles[4] = {DDS::HANDLE_NIL, DDS::HANDLE_NIL, DDS::HANDLE_NIL,
                                        DDS::HANDLE_NIL};
    auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(timeout_s);
    while (!(got >= count && disposed && no_writers) && std::chrono::steady_clock::now() < deadline) {
        Keyed::Shape s;
        DDS::SampleInfo info;
        if (reader->take_next_sample(s, info) != DDS::RETCODE_OK) {
            sleep_ms(20);
            continue;
        }
        if (info.valid_data) {
            CORBA::ULong id = s.id;
            if (id < 1 || id > 3 || std::string(s.message.in()) != MESSAGE || s.index <= last[id] ||
                (s.index - 1) % 3 + 1 != id) {
                std::fprintf(stderr, "opendds: bad sample id %u index %u\n", static_cast<unsigned>(id),
                             static_cast<unsigned>(s.index));
                return 1;
            }
            handles[id] = info.instance_handle;
            last[id] = s.index;
            got++;
            std::printf("opendds: received id %u index %u\n", static_cast<unsigned>(id),
                        static_cast<unsigned>(s.index));
        } else {
            unsigned id = 0;
            for (unsigned i = 1; i <= 3; i++) {
                if (handles[i] == info.instance_handle) {
                    id = i;
                }
            }
            std::printf("opendds: id %u is %s\n", id, state_name(info.instance_state));
            if (id == 2 && info.instance_state == DDS::NOT_ALIVE_DISPOSED_INSTANCE_STATE) {
                disposed = true;
            }
            if (id == 3 && info.instance_state == DDS::NOT_ALIVE_NO_WRITERS_INSTANCE_STATE) {
                no_writers = true;
            }
        }
        std::fflush(stdout);
    }
    if (got < count || !disposed || !no_writers) {
        std::fprintf(stderr, "opendds: got %d of %d samples, disposed %d, no writers %d\n", got, count,
                     disposed, no_writers);
        return 1;
    }
    return 0;
}

int main(int argc, char** argv)
{
    DDS::DomainParticipantFactory_var dpf = factory(argc, argv);
    if (argc != 5) {
        std::fprintf(stderr, "usage: %s pub|sub <domain> <count> <interval_ms|timeout_s>\n", argv[0]);
        return 2;
    }
    DDS::DomainParticipant_var pp = participant(dpf, std::atoi(argv[2]));
    if (!pp) {
        return 1;
    }
    Keyed::ShapeTypeSupport_var ts = new Keyed::ShapeTypeSupportImpl;
    if (ts->register_type(pp, "") != DDS::RETCODE_OK) {
        std::fprintf(stderr, "opendds: register_type failed\n");
        return 1;
    }
    CORBA::String_var type_name = ts->get_type_name();
    DDS::Topic_var topic = pp->create_topic("keyed", type_name, TOPIC_QOS_DEFAULT, 0,
                                            OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    if (!topic) {
        std::fprintf(stderr, "opendds: create_topic failed\n");
        return 1;
    }
    int count = std::atoi(argv[3]), arg = std::atoi(argv[4]);
    int rc = std::string(argv[1]) == "pub" ? publish(pp, topic, count, arg)
                                           : subscribe(pp, topic, count, arg);
    finish(dpf, pp);
    return rc;
}

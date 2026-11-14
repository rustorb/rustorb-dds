// Hello world publisher and subscriber for OpenDDS, written against its
// public API only. Used by interop/run.sh; the same as the Fast DDS one
// (interop/fastdds/hello.cpp), without the content filtered topic.
//
//   hello pub <domain> <count> <interval_ms> [best_effort|reliable|transient_local [size]]
//   hello sub <domain> <count> <timeout_s> [best_effort|reliable|transient_local [size]]
//
// The -DCPSConfigFile argument (run.sh passes interop/opendds/rtps.ini)
// may come anywhere. HELLO_XCDR2=1 makes the writer write XCDR2 instead
// of XCDR; readers take both.

#include "common.h"

#include "HelloWorldTypeSupportImpl.h"

using namespace interop;

static const char* MESSAGE = "Hello, world!";
static bool reliable = false;
static bool transient_local = false;
static long size = -1;

static std::string message(unsigned index)
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

static int publish(DDS::DomainParticipant_ptr pp, DDS::Topic_ptr topic, int count, int interval_ms)
{
    DDS::Publisher_var pub =
        pp->create_publisher(PUBLISHER_QOS_DEFAULT, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    DDS::DataWriterQos qos;
    pub->get_default_datawriter_qos(qos);
    if (reliable) {
        qos.reliability.kind = DDS::RELIABLE_RELIABILITY_QOS;
        qos.history.kind = DDS::KEEP_ALL_HISTORY_QOS;
    } else {
        qos.reliability.kind = DDS::BEST_EFFORT_RELIABILITY_QOS;
    }
    qos.durability.kind =
        transient_local ? DDS::TRANSIENT_LOCAL_DURABILITY_QOS : DDS::VOLATILE_DURABILITY_QOS;
    const char* x2 = std::getenv("HELLO_XCDR2");
    bool xcdr2 = x2 && std::string(x2) == "1";
    set_representation(qos.representation.value, !xcdr2, xcdr2);
    DDS::DataWriter_var w =
        pub->create_datawriter(topic, qos, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    HelloWorld::GreetingDataWriter_var writer = HelloWorld::GreetingDataWriter::_narrow(w);
    if (!writer) {
        std::fprintf(stderr, "opendds: create_datawriter failed\n");
        return 1;
    }
    if (!transient_local) {
        wait_for_reader(writer);
        // Give the reader time to match this writer too.
        sleep_ms(500);
    }
    for (int i = 1; i <= count; i++) {
        HelloWorld::Greeting g;
        g.index = static_cast<CORBA::ULong>(i);
        g.message = message(static_cast<unsigned>(i)).c_str();
        if (writer->write(g, DDS::HANDLE_NIL) != DDS::RETCODE_OK) {
            std::fprintf(stderr, "opendds: write failed\n");
            return 1;
        }
        std::printf("opendds: sent %d\n", i);
        std::fflush(stdout);
        sleep_ms(interval_ms);
    }
    if (transient_local) {
        wait_for_reader(writer);
    }
    if (reliable && !wait_for_acknowledgments(writer)) {
        return 1;
    }
    return 0;
}

static int subscribe(DDS::DomainParticipant_ptr pp, DDS::Topic_ptr topic, int count, int timeout_s)
{
    DDS::Subscriber_var sub =
        pp->create_subscriber(SUBSCRIBER_QOS_DEFAULT, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    DDS::DataReaderQos qos;
    sub->get_default_datareader_qos(qos);
    qos.reliability.kind =
        reliable ? DDS::RELIABLE_RELIABILITY_QOS : DDS::BEST_EFFORT_RELIABILITY_QOS;
    qos.history.kind = DDS::KEEP_ALL_HISTORY_QOS;
    qos.durability.kind =
        transient_local ? DDS::TRANSIENT_LOCAL_DURABILITY_QOS : DDS::VOLATILE_DURABILITY_QOS;
    set_representation(qos.representation.value, true, true);
    if (transient_local) {
        sleep_ms(2000);  // join late
    }
    DDS::DataReader_var r =
        sub->create_datareader(topic, qos, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    HelloWorld::GreetingDataReader_var reader = HelloWorld::GreetingDataReader::_narrow(r);
    if (!reader) {
        std::fprintf(stderr, "opendds: create_datareader failed\n");
        return 1;
    }
    int got = 0;
    auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(timeout_s);
    while (got < count && std::chrono::steady_clock::now() < deadline) {
        HelloWorld::Greeting g;
        DDS::SampleInfo info;
        if (reader->take_next_sample(g, info) != DDS::RETCODE_OK) {
            sleep_ms(20);
            continue;
        }
        if (!info.valid_data) {
            continue;
        }
        std::string m = g.message.in();
        if (m != message(g.index)) {
            std::fprintf(stderr, "opendds: bad message \"%.40s\" (%zu octets) for %u\n", m.c_str(),
                         m.size(), static_cast<unsigned>(g.index));
            return 1;
        }
        std::printf("opendds: received %u\n", static_cast<unsigned>(g.index));
        std::fflush(stdout);
        got++;
        if (reliable && g.index != static_cast<CORBA::ULong>(got)) {
            std::fprintf(stderr, "opendds: expected %d, got %u\n", got, static_cast<unsigned>(g.index));
            return 1;
        }
    }
    if (got < count) {
        std::fprintf(stderr, "opendds: got %d of %d samples\n", got, count);
        return 1;
    }
    return 0;
}

int main(int argc, char** argv)
{
    DDS::DomainParticipantFactory_var dpf = factory(argc, argv);
    if (argc < 5 || argc > 7) {
        std::fprintf(stderr,
                     "usage: %s pub|sub <domain> <count> <interval_ms|timeout_s> "
                     "[best_effort|reliable|transient_local [size]]\n",
                     argv[0]);
        return 2;
    }
    std::string mode = argc >= 6 ? argv[5] : "best_effort";
    transient_local = mode == "transient_local";
    reliable = transient_local || mode == "reliable";
    if (argc == 7) {
        size = std::atol(argv[6]);
    }
    DDS::DomainParticipant_var pp = participant(dpf, std::atoi(argv[2]));
    if (!pp) {
        return 1;
    }
    HelloWorld::GreetingTypeSupport_var ts = new HelloWorld::GreetingTypeSupportImpl;
    if (ts->register_type(pp, "") != DDS::RETCODE_OK) {
        std::fprintf(stderr, "opendds: register_type failed\n");
        return 1;
    }
    CORBA::String_var type_name = ts->get_type_name();
    DDS::Topic_var topic = pp->create_topic("hello_world", type_name, TOPIC_QOS_DEFAULT, 0,
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

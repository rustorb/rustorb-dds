// A recursive type for OpenDDS, written against its public API only.
// Used by interop/run.sh; the same as the tree example of rustorb-dds.
// The type is Tree::Node of interop/idl/Tree.idl.
//
//   tree pub <domain> <count>
//   tree sub <domain> <count> <timeout_s>
//
// The topic is "tree", RELIABLE and KEEP_ALL, written in XCDR2. Sample n
// is the tree n (10n + 1, 10n + 2 (100n)). The publisher waits for a
// reader, writes <count> samples and waits for acknowledgments. The
// subscriber exits with 0 once it has <count> samples, the expected ones
// in order; with 1 on timeout or a wrong sample.

#include "common.h"

#include "TreeTypeSupportImpl.h"

using namespace interop;

template <typename Q>
static void set_qos(Q& qos)
{
    qos.reliability.kind = DDS::RELIABLE_RELIABILITY_QOS;
    qos.history.kind = DDS::KEEP_ALL_HISTORY_QOS;
    set_representation(qos.representation.value, false, true);
}

static Tree::Node sample(int n)
{
    Tree::Node s;
    s.data = n;
    s.children.length(2);
    s.children[0].data = 10 * n + 1;
    s.children[1].data = 10 * n + 2;
    s.children[1].children.length(1);
    s.children[1].children[0].data = 100 * n;
    return s;
}

// The tree as "data (child, child...)".
static std::string text(const Tree::Node& s)
{
    std::string out = std::to_string(s.data);
    if (s.children.length() > 0) {
        out += " (";
        for (CORBA::ULong i = 0; i < s.children.length(); i++) {
            out += (i ? ", " : "") + text(s.children[i]);
        }
        out += ")";
    }
    return out;
}

static int publish(DDS::DomainParticipant_ptr pp, DDS::Topic_ptr topic, int count)
{
    DDS::Publisher_var pub =
        pp->create_publisher(PUBLISHER_QOS_DEFAULT, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    DDS::DataWriterQos qos;
    pub->get_default_datawriter_qos(qos);
    set_qos(qos);
    DDS::DataWriter_var w =
        pub->create_datawriter(topic, qos, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    Tree::NodeDataWriter_var writer = Tree::NodeDataWriter::_narrow(w);
    if (!writer) {
        std::fprintf(stderr, "opendds: create_datawriter failed\n");
        return 1;
    }
    wait_for_reader(writer);
    sleep_ms(500);
    for (int i = 1; i <= count; i++) {
        if (writer->write(sample(i), DDS::HANDLE_NIL) != DDS::RETCODE_OK) {
            std::fprintf(stderr, "opendds: write failed\n");
            return 1;
        }
        std::printf("opendds: sent %d\n", i);
        std::fflush(stdout);
        sleep_ms(20);
    }
    return wait_for_acknowledgments(writer) ? 0 : 1;
}

static int subscribe(DDS::DomainParticipant_ptr pp, DDS::Topic_ptr topic, int count, int timeout_s)
{
    DDS::Subscriber_var sub =
        pp->create_subscriber(SUBSCRIBER_QOS_DEFAULT, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    DDS::DataReaderQos qos;
    sub->get_default_datareader_qos(qos);
    set_qos(qos);
    DDS::DataReader_var r =
        sub->create_datareader(topic, qos, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    Tree::NodeDataReader_var reader = Tree::NodeDataReader::_narrow(r);
    if (!reader) {
        std::fprintf(stderr, "opendds: create_datareader failed\n");
        return 1;
    }
    int got = 0;
    auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(timeout_s);
    while (got < count && std::chrono::steady_clock::now() < deadline) {
        Tree::Node s;
        DDS::SampleInfo info;
        if (reader->take_next_sample(s, info) != DDS::RETCODE_OK) {
            sleep_ms(20);
            continue;
        }
        if (!info.valid_data) {
            continue;
        }
        got++;
        std::string have = text(s), want = text(sample(got));
        if (have != want) {
            std::fprintf(stderr, "opendds: got the tree %s, want %s\n", have.c_str(), want.c_str());
            return 1;
        }
        std::printf("opendds: received %d: %s\n", got, have.c_str());
        std::fflush(stdout);
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
    bool sub = argc >= 2 && std::string(argv[1]) == "sub";
    if (argc != (sub ? 5 : 4)) {
        std::fprintf(stderr,
                     "usage: %s pub <domain> <count>\n"
                     "       %s sub <domain> <count> <timeout_s>\n",
                     argv[0], argv[0]);
        return 2;
    }
    DDS::DomainParticipant_var pp = participant(dpf, std::atoi(argv[2]));
    if (!pp) {
        return 1;
    }
    Tree::NodeTypeSupport_var ts = new Tree::NodeTypeSupportImpl;
    if (ts->register_type(pp, "") != DDS::RETCODE_OK) {
        std::fprintf(stderr, "opendds: register_type failed\n");
        return 1;
    }
    CORBA::String_var type_name = ts->get_type_name();
    DDS::Topic_var topic = pp->create_topic("tree", type_name, TOPIC_QOS_DEFAULT, 0,
                                            OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    if (!topic) {
        std::fprintf(stderr, "opendds: create_topic failed\n");
        return 1;
    }
    int count = std::atoi(argv[3]);
    int rc = sub ? subscribe(pp, topic, count, std::atoi(argv[4])) : publish(pp, topic, count);
    finish(dpf, pp);
    return rc;
}

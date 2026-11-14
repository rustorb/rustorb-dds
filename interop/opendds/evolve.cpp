// Type evolution for OpenDDS, written against its public API only. Used
// by interop/run.sh; the same as interop/cyclonedds/evolve.c. Built
// twice: evolve_v1 with the types of EvolveV1.idl, evolve_v2 with those
// of EvolveV2.idl, which have one more member.
//
//   evolve_vN pub <domain> <count> <appendable|mutable>
//   evolve_vN sub <domain> <count> <timeout_s> <appendable|mutable>
//
// The topic is "evolve_appendable" or "evolve_mutable", RELIABLE and
// KEEP_ALL, written in XCDR2. The publisher waits for a reader, writes
// <count> samples (number 1, 2, 3..., text "evolve <number>", extra
// 100 + number in version 2) and waits for acknowledgments. The
// subscriber exits with 0 once it has <count> samples with the expected
// number and text in order; with 1 on timeout or a wrong sample.

#include "common.h"

#ifdef EVOLVE_V2
#include "EvolveV2TypeSupportImpl.h"
#else
#include "EvolveV1TypeSupportImpl.h"
#endif

using namespace interop;

template <typename T>
struct Dds;
template <>
struct Dds<Evolve::Appendable> {
    using TypeSupportImpl = Evolve::AppendableTypeSupportImpl;
    using TypeSupport_var = Evolve::AppendableTypeSupport_var;
    using Writer = Evolve::AppendableDataWriter;
    using Writer_var = Evolve::AppendableDataWriter_var;
    using Reader = Evolve::AppendableDataReader;
    using Reader_var = Evolve::AppendableDataReader_var;
    static const char* topic() { return "evolve_appendable"; }
};
template <>
struct Dds<Evolve::Mutable> {
    using TypeSupportImpl = Evolve::MutableTypeSupportImpl;
    using TypeSupport_var = Evolve::MutableTypeSupport_var;
    using Writer = Evolve::MutableDataWriter;
    using Writer_var = Evolve::MutableDataWriter_var;
    using Reader = Evolve::MutableDataReader;
    using Reader_var = Evolve::MutableDataReader_var;
    static const char* topic() { return "evolve_mutable"; }
};

template <typename Q>
static void set_qos(Q& qos)
{
    qos.reliability.kind = DDS::RELIABLE_RELIABILITY_QOS;
    qos.history.kind = DDS::KEEP_ALL_HISTORY_QOS;
    set_representation(qos.representation.value, false, true);
}

template <typename T>
static int publish(DDS::DomainParticipant_ptr pp, DDS::Topic_ptr topic, int count)
{
    DDS::Publisher_var pub =
        pp->create_publisher(PUBLISHER_QOS_DEFAULT, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    DDS::DataWriterQos qos;
    pub->get_default_datawriter_qos(qos);
    set_qos(qos);
    DDS::DataWriter_var w =
        pub->create_datawriter(topic, qos, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    typename Dds<T>::Writer_var writer = Dds<T>::Writer::_narrow(w);
    if (!writer) {
        std::fprintf(stderr, "opendds: create_datawriter failed\n");
        return 1;
    }
    wait_for_reader(writer);
    sleep_ms(500);
    for (int i = 1; i <= count; i++) {
        T s;
        s.number = i;
        s.text = ("evolve " + std::to_string(i)).c_str();
#ifdef EVOLVE_V2
        s.extra = 100 + i;
#endif
        if (writer->write(s, DDS::HANDLE_NIL) != DDS::RETCODE_OK) {
            std::fprintf(stderr, "opendds: write failed\n");
            return 1;
        }
        std::printf("opendds: sent %d\n", i);
        std::fflush(stdout);
        sleep_ms(20);
    }
    return wait_for_acknowledgments(writer) ? 0 : 1;
}

template <typename T>
static int subscribe(DDS::DomainParticipant_ptr pp, DDS::Topic_ptr topic, int count, int timeout_s)
{
    DDS::Subscriber_var sub =
        pp->create_subscriber(SUBSCRIBER_QOS_DEFAULT, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    DDS::DataReaderQos qos;
    sub->get_default_datareader_qos(qos);
    set_qos(qos);
    DDS::DataReader_var r =
        sub->create_datareader(topic, qos, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    typename Dds<T>::Reader_var reader = Dds<T>::Reader::_narrow(r);
    if (!reader) {
        std::fprintf(stderr, "opendds: create_datareader failed\n");
        return 1;
    }
    int got = 0;
    auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(timeout_s);
    while (got < count && std::chrono::steady_clock::now() < deadline) {
        T s;
        DDS::SampleInfo info;
        if (reader->take_next_sample(s, info) != DDS::RETCODE_OK) {
            sleep_ms(20);
            continue;
        }
        if (!info.valid_data) {
            continue;
        }
        std::string want = "evolve " + std::to_string(got + 1);
        if (s.number != got + 1 || want != s.text.in()) {
            std::fprintf(stderr, "opendds: got id %d text \"%s\", want %d \"%s\"\n",
                         static_cast<int>(s.number), s.text.in(), got + 1, want.c_str());
            return 1;
        }
        got++;
        std::printf("opendds: received %d\n", static_cast<int>(s.number));
        std::fflush(stdout);
    }
    if (got < count) {
        std::fprintf(stderr, "opendds: got %d of %d samples\n", got, count);
        return 1;
    }
    return 0;
}

template <typename T>
static int run(DDS::DomainParticipant_ptr pp, bool sub, int count, int timeout_s)
{
    typename Dds<T>::TypeSupport_var ts = new typename Dds<T>::TypeSupportImpl;
    if (ts->register_type(pp, "") != DDS::RETCODE_OK) {
        std::fprintf(stderr, "opendds: register_type failed\n");
        return 1;
    }
    CORBA::String_var type_name = ts->get_type_name();
    DDS::Topic_var topic = pp->create_topic(Dds<T>::topic(), type_name, TOPIC_QOS_DEFAULT, 0,
                                            OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    if (!topic) {
        std::fprintf(stderr, "opendds: create_topic failed\n");
        return 1;
    }
    return sub ? subscribe<T>(pp, topic, count, timeout_s) : publish<T>(pp, topic, count);
}

int main(int argc, char** argv)
{
    DDS::DomainParticipantFactory_var dpf = factory(argc, argv);
    bool sub = argc >= 2 && std::string(argv[1]) == "sub";
    if (argc != (sub ? 6 : 5)) {
        std::fprintf(stderr,
                     "usage: %s pub <domain> <count> <appendable|mutable>\n"
                     "       %s sub <domain> <count> <timeout_s> <appendable|mutable>\n",
                     argv[0], argv[0]);
        return 2;
    }
    DDS::DomainParticipant_var pp = participant(dpf, std::atoi(argv[2]));
    if (!pp) {
        return 1;
    }
    int count = std::atoi(argv[3]);
    int timeout_s = sub ? std::atoi(argv[4]) : 0;
    bool mutable_ = std::string(argv[sub ? 5 : 4]) == "mutable";
    int rc = mutable_ ? run<Evolve::Mutable>(pp, sub, count, timeout_s)
                      : run<Evolve::Appendable>(pp, sub, count, timeout_s);
    finish(dpf, pp);
    return rc;
}

// What the OpenDDS programs of interop/run.sh share. Written against
// OpenDDS's public API (the DCPS API of DDS 1.4 in the classic IDL to C++
// mapping) only.
#ifndef RUSTORB_INTEROP_OPENDDS_COMMON_H
#define RUSTORB_INTEROP_OPENDDS_COMMON_H

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <thread>

#include <dds/DCPS/Marked_Default_Qos.h>
#include <dds/DCPS/RTPS/RtpsDiscovery.h>
#include <dds/DCPS/Service_Participant.h>
#include <dds/DCPS/transport/rtps_udp/RtpsUdp.h>
#include <dds/DdsDcpsInfrastructureC.h>

namespace interop {

inline void sleep_ms(long ms)
{
    std::this_thread::sleep_for(std::chrono::milliseconds(ms));
}

// Waits up to 30 s for a matched reader.
inline void wait_for_reader(DDS::DataWriter_ptr writer)
{
    for (int i = 0; i < 300; i++) {
        DDS::PublicationMatchedStatus st;
        writer->get_publication_matched_status(st);
        if (st.current_count > 0) {
            return;
        }
        sleep_ms(100);
    }
}

inline bool wait_for_acknowledgments(DDS::DataWriter_ptr writer)
{
    DDS::Duration_t d = {30, 0};
    if (writer->wait_for_acknowledgments(d) != DDS::RETCODE_OK) {
        std::fprintf(stderr, "opendds: wait_for_acknowledgments failed\n");
        return false;
    }
    std::printf("opendds: all acknowledged\n");
    std::fflush(stdout);
    return true;
}

// XCDR (version 1) and XCDR2 for readers, or one of them.
inline void set_representation(DDS::DataRepresentationIdSeq& seq, bool xcdr1, bool xcdr2)
{
    seq.length(0);
    if (xcdr1) {
        seq.length(seq.length() + 1);
        seq[seq.length() - 1] = DDS::XCDR_DATA_REPRESENTATION;
    }
    if (xcdr2) {
        seq.length(seq.length() + 1);
        seq[seq.length() - 1] = DDS::XCDR2_DATA_REPRESENTATION;
    }
}

// The participant factory, set up from the -DCPS* arguments (the
// configuration file, rtps.ini), which it removes from argv.
inline DDS::DomainParticipantFactory_ptr factory(int& argc, char** argv)
{
    return TheParticipantFactoryWithArgs(argc, argv);
}

inline DDS::DomainParticipant_ptr participant(DDS::DomainParticipantFactory_ptr f, int domain)
{
    DDS::DomainParticipant_ptr p = f->create_participant(
        domain, PARTICIPANT_QOS_DEFAULT, 0, OpenDDS::DCPS::DEFAULT_STATUS_MASK);
    if (!p) {
        std::fprintf(stderr, "opendds: create_participant failed\n");
    }
    return p;
}

inline void finish(DDS::DomainParticipantFactory_ptr f, DDS::DomainParticipant_ptr p)
{
    p->delete_contained_entities();
    f->delete_participant(p);
    TheServiceParticipant->shutdown();
}

}  // namespace interop

#endif

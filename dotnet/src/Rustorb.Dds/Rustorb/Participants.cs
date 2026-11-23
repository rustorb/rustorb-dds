// rustorb's extensions that are not in the PSM.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using Omg.Dds.Domain;

namespace Rustorb.Dds;

public static class Participants
{
    /// <summary>A participant that uses only the loopback interface (for tests).</summary>
    public static DomainParticipant Localhost(int domainId, DomainParticipantQos? qos = null) =>
        new(domainId, qos, 1u);
}

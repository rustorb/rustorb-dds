// The communication statuses (DDS 1.4, 2.2.4.1). Getting a status resets
// its change counts.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections.Immutable;
using Rustorb.Dds.Native;

namespace Omg.Dds.Core.Status;

/// <summary>StatusKind bits (DDS 1.4, 2.2.4.1).</summary>
[Flags]
public enum StatusKind : uint
{
    None = 0,
    InconsistentTopic = 1u << 0,
    OfferedDeadlineMissed = 1u << 1,
    RequestedDeadlineMissed = 1u << 2,
    OfferedIncompatibleQos = 1u << 5,
    RequestedIncompatibleQos = 1u << 6,
    SampleLost = 1u << 7,
    SampleRejected = 1u << 8,
    DataOnReaders = 1u << 9,
    DataAvailable = 1u << 10,
    LivelinessLost = 1u << 11,
    LivelinessChanged = 1u << 12,
    PublicationMatched = 1u << 13,
    SubscriptionMatched = 1u << 14,
    All = 0xffffffffu,
}

public sealed record PublicationMatchedStatus(
    int TotalCount, int TotalCountChange, int CurrentCount, int CurrentCountChange,
    InstanceHandle LastSubscriptionHandle)
{
    internal static PublicationMatchedStatus Of(in RddsMatchedStatus s) =>
        new(s.total_count, s.total_count_change, s.current_count, s.current_count_change,
            InstanceHandle.Of(s.last_handle));
}

public sealed record SubscriptionMatchedStatus(
    int TotalCount, int TotalCountChange, int CurrentCount, int CurrentCountChange,
    InstanceHandle LastPublicationHandle)
{
    internal static SubscriptionMatchedStatus Of(in RddsMatchedStatus s) =>
        new(s.total_count, s.total_count_change, s.current_count, s.current_count_change,
            InstanceHandle.Of(s.last_handle));
}

/// <summary>The number of incompatibilities found for a policy.</summary>
public sealed record QosPolicyCount(int PolicyId, int Count);

public sealed record OfferedIncompatibleQosStatus(
    int TotalCount, int TotalCountChange, int LastPolicyId, ImmutableArray<QosPolicyCount> Policies)
{
    internal static OfferedIncompatibleQosStatus Of(in RddsIncompatibleQosStatus s) =>
        new(s.total_count, s.total_count_change, s.last_policy_id, Counts(s));

    internal static unsafe ImmutableArray<QosPolicyCount> Counts(in RddsIncompatibleQosStatus s)
    {
        var b = ImmutableArray.CreateBuilder<QosPolicyCount>();
        int n = (int)Math.Min(s.policies_len, RddsIncompatibleQosStatus.MAX_POLICIES);
        fixed (int* p = s.policies)
        {
            for (int i = 0; i < n; i++)
            {
                b.Add(new QosPolicyCount(p[2 * i], p[2 * i + 1]));
            }
        }
        return b.ToImmutable();
    }
}

public sealed record RequestedIncompatibleQosStatus(
    int TotalCount, int TotalCountChange, int LastPolicyId, ImmutableArray<QosPolicyCount> Policies)
{
    internal static RequestedIncompatibleQosStatus Of(in RddsIncompatibleQosStatus s) =>
        new(s.total_count, s.total_count_change, s.last_policy_id, OfferedIncompatibleQosStatus.Counts(s));
}

public sealed record LivelinessLostStatus(int TotalCount, int TotalCountChange)
{
    internal static LivelinessLostStatus Of(in RddsCountStatus s) => new(s.total_count, s.total_count_change);
}

public sealed record LivelinessChangedStatus(
    int AliveCount, int NotAliveCount, int AliveCountChange, int NotAliveCountChange,
    InstanceHandle LastPublicationHandle)
{
    internal static LivelinessChangedStatus Of(in RddsLivelinessChangedStatus s) =>
        new(s.alive_count, s.not_alive_count, s.alive_count_change, s.not_alive_count_change,
            InstanceHandle.Of(s.last_publication_handle));
}

public sealed record OfferedDeadlineMissedStatus(int TotalCount, int TotalCountChange, InstanceHandle LastInstanceHandle)
{
    internal static OfferedDeadlineMissedStatus Of(in RddsDeadlineMissedStatus s) =>
        new(s.total_count, s.total_count_change, InstanceHandle.Of(s.last_instance_handle));
}

public sealed record RequestedDeadlineMissedStatus(int TotalCount, int TotalCountChange, InstanceHandle LastInstanceHandle)
{
    internal static RequestedDeadlineMissedStatus Of(in RddsDeadlineMissedStatus s) =>
        new(s.total_count, s.total_count_change, InstanceHandle.Of(s.last_instance_handle));
}

public sealed record SampleLostStatus(int TotalCount, int TotalCountChange)
{
    internal static SampleLostStatus Of(in RddsCountStatus s) => new(s.total_count, s.total_count_change);
}

public enum SampleRejectedStatusKind
{
    NotRejected = 0,
    RejectedByInstancesLimit = 1,
    RejectedBySamplesLimit = 2,
    RejectedBySamplesPerInstanceLimit = 3,
}

public sealed record SampleRejectedStatus(
    int TotalCount, int TotalCountChange, SampleRejectedStatusKind LastReason, InstanceHandle LastInstanceHandle)
{
    internal static SampleRejectedStatus Of(in RddsSampleRejectedStatus s) =>
        new(s.total_count, s.total_count_change, (SampleRejectedStatusKind)s.last_reason,
            InstanceHandle.Of(s.last_instance_handle));
}

public sealed record InconsistentTopicStatus(int TotalCount, int TotalCountChange)
{
    internal static InconsistentTopicStatus Of(in RddsCountStatus s) => new(s.total_count, s.total_count_change);
}

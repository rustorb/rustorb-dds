// The C# layouts of the plain structs of rustorb/dds.h. A C bool is one
// octet (byte here); pointers are nint.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System.Runtime.InteropServices;

namespace Rustorb.Dds.Native;

[StructLayout(LayoutKind.Sequential)]
internal struct RddsDuration
{
    internal int sec;
    internal uint nanosec;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsTime
{
    internal int sec;
    internal uint nanosec;
}

[StructLayout(LayoutKind.Sequential)]
internal unsafe struct RddsGuid
{
    internal fixed byte value[16];
}

[StructLayout(LayoutKind.Sequential)]
internal unsafe struct RddsInstanceHandle
{
    internal fixed byte value[16];
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsWriterQos
{
    internal int reliability_kind;
    internal RddsDuration max_blocking_time;
    internal int history_kind;
    internal int history_depth;
    internal int max_samples;
    internal int max_instances;
    internal int max_samples_per_instance;
    internal int durability_kind;
    internal RddsDuration deadline_period;
    internal int liveliness_kind;
    internal RddsDuration lease_duration;
    internal nint user_data;
    internal nuint user_data_len;
    internal nint data_representation;
    internal nuint data_representation_len;
    internal byte autodispose_unregistered_instances;
    internal RddsDuration service_cleanup_delay;
    internal int service_history_kind;
    internal int service_history_depth;
    internal int service_max_samples;
    internal int service_max_instances;
    internal int service_max_samples_per_instance;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsReaderQos
{
    internal int reliability_kind;
    internal RddsDuration max_blocking_time;
    internal int history_kind;
    internal int history_depth;
    internal int max_samples;
    internal int max_instances;
    internal int max_samples_per_instance;
    internal int durability_kind;
    internal RddsDuration deadline_period;
    internal int liveliness_kind;
    internal RddsDuration lease_duration;
    internal nint user_data;
    internal nuint user_data_len;
    internal nint data_representation;
    internal nuint data_representation_len;
    internal int type_consistency_kind;
    internal byte ignore_sequence_bounds;
    internal byte ignore_string_bounds;
    internal byte ignore_member_names;
    internal byte prevent_type_widening;
    internal byte force_type_validation;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsGroupQos
{
    internal nint partition;
    internal nuint partition_len;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsProperty
{
    internal nint name;
    internal nint value;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsParticipantQos
{
    internal nint user_data;
    internal nuint user_data_len;
    internal nint property;
    internal nuint property_len;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsMatchedStatus
{
    internal int total_count;
    internal int total_count_change;
    internal int current_count;
    internal int current_count_change;
    internal RddsGuid last_handle;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsQosPolicyCount
{
    internal int policy_id;
    internal int count;
}

[StructLayout(LayoutKind.Sequential)]
internal unsafe struct RddsIncompatibleQosStatus
{
    internal const int MAX_POLICIES = 32;
    internal int total_count;
    internal int total_count_change;
    internal int last_policy_id;
    internal uint policies_len;
    // rdds_qos_policy_count policies[RDDS_MAX_POLICIES]: policy_id, count.
    internal fixed int policies[MAX_POLICIES * 2];
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsLivelinessChangedStatus
{
    internal int alive_count;
    internal int not_alive_count;
    internal int alive_count_change;
    internal int not_alive_count_change;
    internal RddsGuid last_publication_handle;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsCountStatus
{
    internal int total_count;
    internal int total_count_change;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsDeadlineMissedStatus
{
    internal int total_count;
    internal int total_count_change;
    internal RddsInstanceHandle last_instance_handle;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsSampleRejectedStatus
{
    internal int total_count;
    internal int total_count_change;
    internal int last_reason;
    internal RddsInstanceHandle last_instance_handle;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsWriterListener
{
    internal nint self;
    internal nint on_offered_deadline_missed;
    internal nint on_offered_incompatible_qos;
    internal nint on_liveliness_lost;
    internal nint on_publication_matched;
    internal nint release;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsReaderListener
{
    internal nint self;
    internal nint on_requested_deadline_missed;
    internal nint on_requested_incompatible_qos;
    internal nint on_sample_rejected;
    internal nint on_liveliness_changed;
    internal nint on_data_available;
    internal nint on_subscription_matched;
    internal nint on_sample_lost;
    internal nint release;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsEndpointInfo
{
    internal RddsGuid guid;
    internal nint topic_name;
    internal nint type_name;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsListener
{
    internal nint self;
    internal nint on_offered_deadline_missed;
    internal nint on_offered_incompatible_qos;
    internal nint on_liveliness_lost;
    internal nint on_publication_matched;
    internal nint on_data_on_readers;
    internal nint on_requested_deadline_missed;
    internal nint on_requested_incompatible_qos;
    internal nint on_sample_rejected;
    internal nint on_liveliness_changed;
    internal nint on_data_available;
    internal nint on_subscription_matched;
    internal nint on_sample_lost;
    internal nint on_inconsistent_topic;
    internal nint release;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsSampleInfo
{
    internal uint sample_state;
    internal uint view_state;
    internal uint instance_state;
    internal int disposed_generation_count;
    internal int no_writers_generation_count;
    internal int sample_rank;
    internal int generation_rank;
    internal int absolute_generation_rank;
    internal RddsTime source_timestamp;
    internal RddsInstanceHandle instance_handle;
    internal RddsGuid publication_handle;
    internal long sequence_number;
    internal byte valid_data;
}

[StructLayout(LayoutKind.Sequential)]
internal struct RddsSampleIdentity
{
    internal RddsGuid writer;
    internal long sequence_number;
}

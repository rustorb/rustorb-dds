// The QoS of the entities: immutable records of policies. Start from the
// default (Publisher.DefaultDataWriterQos...) and change what you need with
// `with`. The QoS of an entity is fixed when it is created.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.Runtime.InteropServices;
using Omg.Dds.Core;
using Omg.Dds.Core.Policy;
using Rustorb.Dds;
using Rustorb.Dds.Native;

namespace Omg.Dds.Pub
{
    public sealed record DataWriterQos(
        Reliability Reliability,
        History History,
        ResourceLimits ResourceLimits,
        Durability Durability,
        Deadline Deadline,
        Liveliness Liveliness,
        UserData UserData,
        DataRepresentation DataRepresentation,
        WriterDataLifecycle WriterDataLifecycle,
        DurabilityService DurabilityService)
    {
        /// <summary>rustorb-dds's default (rdds_writer_qos_default).</summary>
        public static unsafe DataWriterQos Default
        {
            get
            {
                Library.Check();
                RddsWriterQos q;
                Errors.Check(Ffi.rdds_writer_qos_default(&q));
                return new DataWriterQos(
                    new Reliability((ReliabilityKind)q.reliability_kind, Duration.Of(q.max_blocking_time)),
                    new History((HistoryKind)q.history_kind, q.history_depth),
                    new ResourceLimits(q.max_samples, q.max_instances, q.max_samples_per_instance),
                    new Durability((DurabilityKind)q.durability_kind),
                    new Deadline(Duration.Of(q.deadline_period)),
                    new Liveliness((LivelinessKind)q.liveliness_kind, Duration.Of(q.lease_duration)),
                    new UserData(QosNative.Bytes(q.user_data, q.user_data_len)),
                    new DataRepresentation(QosNative.Shorts(q.data_representation, q.data_representation_len)),
                    new WriterDataLifecycle(q.autodispose_unregistered_instances != 0),
                    new DurabilityService(Duration.Of(q.service_cleanup_delay), (HistoryKind)q.service_history_kind,
                        q.service_history_depth, q.service_max_samples, q.service_max_instances,
                        q.service_max_samples_per_instance));
            }
        }

        internal unsafe T With<T>(QosNative.Fn<RddsWriterQos, T> f)
        {
            byte[] ud = UserData.Value.IsDefault ? Array.Empty<byte>() : UserData.Value.AsSpan().ToArray();
            short[] dr = DataRepresentation.Value.IsDefault ? Array.Empty<short>() : DataRepresentation.Value.AsSpan().ToArray();
            fixed (byte* u = ud)
            fixed (short* d = dr)
            {
                var q = new RddsWriterQos
                {
                    reliability_kind = (int)Reliability.Kind,
                    max_blocking_time = Reliability.MaxBlockingTime.Native,
                    history_kind = (int)History.Kind,
                    history_depth = History.Depth,
                    max_samples = ResourceLimits.MaxSamples,
                    max_instances = ResourceLimits.MaxInstances,
                    max_samples_per_instance = ResourceLimits.MaxSamplesPerInstance,
                    durability_kind = (int)Durability.Kind,
                    deadline_period = Deadline.Period.Native,
                    liveliness_kind = (int)Liveliness.Kind,
                    lease_duration = Liveliness.LeaseDuration.Native,
                    user_data = (nint)u,
                    user_data_len = (nuint)ud.Length,
                    data_representation = (nint)d,
                    data_representation_len = (nuint)dr.Length,
                    autodispose_unregistered_instances = (byte)(WriterDataLifecycle.AutodisposeUnregisteredInstances ? 1 : 0),
                    service_cleanup_delay = DurabilityService.ServiceCleanupDelay.Native,
                    service_history_kind = (int)DurabilityService.HistoryKind,
                    service_history_depth = DurabilityService.HistoryDepth,
                    service_max_samples = DurabilityService.MaxSamples,
                    service_max_instances = DurabilityService.MaxInstances,
                    service_max_samples_per_instance = DurabilityService.MaxSamplesPerInstance,
                };
                return f(&q);
            }
        }
    }

    /// <summary>The QoS of a publisher: its partitions.</summary>
    public sealed record PublisherQos(Partition Partition)
    {
        public static readonly PublisherQos Default = new(new Partition(ImmutableArray<string>.Empty));
    }
}

namespace Omg.Dds.Sub
{
    public sealed record DataReaderQos(
        Reliability Reliability,
        History History,
        ResourceLimits ResourceLimits,
        Durability Durability,
        Deadline Deadline,
        Liveliness Liveliness,
        UserData UserData,
        DataRepresentation DataRepresentation,
        TypeConsistencyEnforcement TypeConsistency)
    {
        /// <summary>rustorb-dds's default (rdds_reader_qos_default).</summary>
        public static unsafe DataReaderQos Default
        {
            get
            {
                Library.Check();
                RddsReaderQos q;
                Errors.Check(Ffi.rdds_reader_qos_default(&q));
                return new DataReaderQos(
                    new Reliability((ReliabilityKind)q.reliability_kind, Duration.Of(q.max_blocking_time)),
                    new History((HistoryKind)q.history_kind, q.history_depth),
                    new ResourceLimits(q.max_samples, q.max_instances, q.max_samples_per_instance),
                    new Durability((DurabilityKind)q.durability_kind),
                    new Deadline(Duration.Of(q.deadline_period)),
                    new Liveliness((LivelinessKind)q.liveliness_kind, Duration.Of(q.lease_duration)),
                    new UserData(QosNative.Bytes(q.user_data, q.user_data_len)),
                    new DataRepresentation(QosNative.Shorts(q.data_representation, q.data_representation_len)),
                    new TypeConsistencyEnforcement((TypeConsistencyKind)q.type_consistency_kind,
                        q.ignore_sequence_bounds != 0, q.ignore_string_bounds != 0, q.ignore_member_names != 0,
                        q.prevent_type_widening != 0, q.force_type_validation != 0));
            }
        }

        internal unsafe T With<T>(QosNative.Fn<RddsReaderQos, T> f)
        {
            byte[] ud = UserData.Value.IsDefault ? Array.Empty<byte>() : UserData.Value.AsSpan().ToArray();
            short[] dr = DataRepresentation.Value.IsDefault ? Array.Empty<short>() : DataRepresentation.Value.AsSpan().ToArray();
            fixed (byte* u = ud)
            fixed (short* d = dr)
            {
                var q = new RddsReaderQos
                {
                    reliability_kind = (int)Reliability.Kind,
                    max_blocking_time = Reliability.MaxBlockingTime.Native,
                    history_kind = (int)History.Kind,
                    history_depth = History.Depth,
                    max_samples = ResourceLimits.MaxSamples,
                    max_instances = ResourceLimits.MaxInstances,
                    max_samples_per_instance = ResourceLimits.MaxSamplesPerInstance,
                    durability_kind = (int)Durability.Kind,
                    deadline_period = Deadline.Period.Native,
                    liveliness_kind = (int)Liveliness.Kind,
                    lease_duration = Liveliness.LeaseDuration.Native,
                    user_data = (nint)u,
                    user_data_len = (nuint)ud.Length,
                    data_representation = (nint)d,
                    data_representation_len = (nuint)dr.Length,
                    type_consistency_kind = (int)TypeConsistency.Kind,
                    ignore_sequence_bounds = (byte)(TypeConsistency.IgnoreSequenceBounds ? 1 : 0),
                    ignore_string_bounds = (byte)(TypeConsistency.IgnoreStringBounds ? 1 : 0),
                    ignore_member_names = (byte)(TypeConsistency.IgnoreMemberNames ? 1 : 0),
                    prevent_type_widening = (byte)(TypeConsistency.PreventTypeWidening ? 1 : 0),
                    force_type_validation = (byte)(TypeConsistency.ForceTypeValidation ? 1 : 0),
                };
                return f(&q);
            }
        }
    }

    /// <summary>The QoS of a subscriber: its partitions.</summary>
    public sealed record SubscriberQos(Partition Partition)
    {
        public static readonly SubscriberQos Default = new(new Partition(ImmutableArray<string>.Empty));
    }
}

namespace Omg.Dds.Domain
{
    /// <summary>The QoS of a participant.</summary>
    public sealed record DomainParticipantQos(UserData UserData, Property Property)
    {
        public static readonly DomainParticipantQos Default =
            new(new UserData(ImmutableArray<byte>.Empty), Property.Empty);
    }
}

namespace Rustorb.Dds.Native
{
    internal static unsafe class QosNative
    {
        internal delegate T Fn<Q, T>(Q* q) where Q : unmanaged;

        internal static byte[] Bytes(nint p, nuint n) =>
            n == 0 ? Array.Empty<byte>() : new ReadOnlySpan<byte>((void*)p, checked((int)n)).ToArray();

        internal static short[] Shorts(nint p, nuint n) =>
            n == 0 ? Array.Empty<short>() : new ReadOnlySpan<short>((void*)p, checked((int)n)).ToArray();

        /// <summary>Calls f with the rdds_group_qos of partitions.</summary>
        internal static T Group<T>(Partition? partition, Fn<RddsGroupQos, T> f)
        {
            ImmutableArray<string> names = partition is null || partition.Name.IsDefault
                ? ImmutableArray<string>.Empty : partition.Name;
            var pins = new List<GCHandle>();
            try
            {
                nint[] ptrs = new nint[names.Length];
                for (int i = 0; i < names.Length; i++)
                {
                    GCHandle h = GCHandle.Alloc(Utf8.Z(names[i]), GCHandleType.Pinned);
                    pins.Add(h);
                    ptrs[i] = h.AddrOfPinnedObject();
                }
                fixed (nint* p = ptrs)
                {
                    var q = new RddsGroupQos { partition = (nint)p, partition_len = (nuint)ptrs.Length };
                    return f(&q);
                }
            }
            finally
            {
                foreach (GCHandle h in pins)
                {
                    h.Free();
                }
            }
        }

        /// <summary>Calls f with the rdds_participant_qos of qos.</summary>
        internal static T Participant<T>(Omg.Dds.Domain.DomainParticipantQos qos, Fn<RddsParticipantQos, T> f)
        {
            var pins = new List<GCHandle>();
            try
            {
                var props = new RddsProperty[qos.Property.Value.Count];
                int i = 0;
                foreach (KeyValuePair<string, string> kv in qos.Property.Value)
                {
                    GCHandle n = GCHandle.Alloc(Utf8.Z(kv.Key), GCHandleType.Pinned);
                    pins.Add(n);
                    GCHandle v = GCHandle.Alloc(Utf8.Z(kv.Value), GCHandleType.Pinned);
                    pins.Add(v);
                    props[i++] = new RddsProperty { name = n.AddrOfPinnedObject(), value = v.AddrOfPinnedObject() };
                }
                byte[] ud = qos.UserData.Value.IsDefault ? Array.Empty<byte>() : qos.UserData.Value.AsSpan().ToArray();
                fixed (byte* u = ud)
                fixed (RddsProperty* p = props)
                {
                    var q = new RddsParticipantQos
                    {
                        user_data = (nint)u,
                        user_data_len = (nuint)ud.Length,
                        property = (nint)p,
                        property_len = (nuint)props.Length,
                    };
                    return f(&q);
                }
            }
            finally
            {
                foreach (GCHandle h in pins)
                {
                    h.Free();
                }
            }
        }
    }
}

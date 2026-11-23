// The QoS policies rustorb-dds implements (DDS 1.4, 2.2.3), as immutable
// records: change one with `with` (policy with { Depth = 10 }).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections.Generic;
using System.Linq;
using System.Collections.Immutable;

namespace Omg.Dds.Core.Policy;

public enum ReliabilityKind { BestEffort = 0, Reliable = 1 }

public enum HistoryKind { KeepLast = 0, KeepAll = 1 }

public enum DurabilityKind { Volatile = 0, TransientLocal = 1, Transient = 2, Persistent = 3 }

public enum LivelinessKind { Automatic = 0, ManualByParticipant = 1, ManualByTopic = 2 }

public enum TypeConsistencyKind { DisallowTypeCoercion = 0, AllowTypeCoercion = 1 }

/// <summary>The QosPolicyId_t values (DDS 1.4, 2.2.3, and DDS-XTypes).</summary>
public static class QosPolicyId
{
    public const int UserData = 1;
    public const int Durability = 2;
    public const int Presentation = 3;
    public const int Deadline = 4;
    public const int LatencyBudget = 5;
    public const int Ownership = 6;
    public const int OwnershipStrength = 7;
    public const int Liveliness = 8;
    public const int TimeBasedFilter = 9;
    public const int Partition = 10;
    public const int Reliability = 11;
    public const int DestinationOrder = 12;
    public const int History = 13;
    public const int ResourceLimits = 14;
    public const int EntityFactory = 15;
    public const int WriterDataLifecycle = 16;
    public const int ReaderDataLifecycle = 17;
    public const int TopicData = 18;
    public const int GroupData = 19;
    public const int TransportPriority = 20;
    public const int Lifespan = 21;
    public const int DurabilityService = 22;
    public const int DataRepresentation = 23;
    public const int TypeConsistencyEnforcement = 24;
}

/// <summary>The data representations (DDS-XTypes 7.6.3.1.1).</summary>
public static class DataRepresentationId
{
    public const short Xcdr = 0;
    public const short Xml = 1;
    public const short Xcdr2 = 2;
}

public sealed record Reliability(ReliabilityKind Kind, Duration MaxBlockingTime)
{
    public static Reliability BestEffort() => new(ReliabilityKind.BestEffort, Duration.FromMilliseconds(100));
    public static Reliability Reliable() => new(ReliabilityKind.Reliable, Duration.FromMilliseconds(100));
}

public sealed record History(HistoryKind Kind, int Depth)
{
    public static History KeepLast(int depth) => new(HistoryKind.KeepLast, depth);
    public static History KeepAll() => new(HistoryKind.KeepAll, 1);
}

/// <summary>LENGTH_UNLIMITED (-1) for no limit.</summary>
public sealed record ResourceLimits(int MaxSamples, int MaxInstances, int MaxSamplesPerInstance)
{
    public const int LengthUnlimited = -1;
}

public sealed record Durability(DurabilityKind Kind)
{
    public static Durability Volatile() => new(DurabilityKind.Volatile);
    public static Durability TransientLocal() => new(DurabilityKind.TransientLocal);
    public static Durability Transient() => new(DurabilityKind.Transient);
    public static Durability Persistent() => new(DurabilityKind.Persistent);
}

public sealed record Deadline(Duration Period);

public sealed record Liveliness(LivelinessKind Kind, Duration LeaseDuration);

public sealed record UserData(ImmutableArray<byte> Value)
{
    public UserData(IEnumerable<byte> value) : this(value.ToImmutableArray()) { }

    public bool Equals(UserData? other) => other is not null && Seq.Equal(Value, other.Value);

    public override int GetHashCode() => Seq.Hash(Value);
}

/// <summary>The data representations, the first preferred (a writer uses it).</summary>
public sealed record DataRepresentation(ImmutableArray<short> Value)
{
    public DataRepresentation(params short[] value) : this(value.ToImmutableArray()) { }

    public bool Equals(DataRepresentation? other) => other is not null && Seq.Equal(Value, other.Value);

    public override int GetHashCode() => Seq.Hash(Value);
}

public sealed record WriterDataLifecycle(bool AutodisposeUnregisteredInstances);

/// <summary>What a TRANSIENT or PERSISTENT writer's data the service keeps
/// after the writer is deleted.</summary>
public sealed record DurabilityService(
    Duration ServiceCleanupDelay,
    HistoryKind HistoryKind,
    int HistoryDepth,
    int MaxSamples,
    int MaxInstances,
    int MaxSamplesPerInstance);

/// <summary>TYPE_CONSISTENCY_ENFORCEMENT (DDS-XTypes 7.6.3.4).</summary>
public sealed record TypeConsistencyEnforcement(
    TypeConsistencyKind Kind,
    bool IgnoreSequenceBounds,
    bool IgnoreStringBounds,
    bool IgnoreMemberNames,
    bool PreventTypeWidening,
    bool ForceTypeValidation);

/// <summary>The partitions of a publisher or subscriber.</summary>
public sealed record Partition(ImmutableArray<string> Name)
{
    public Partition(params string[] name) : this(name.ToImmutableArray()) { }

    public bool Equals(Partition? other) => other is not null && Seq.Equal(Name, other.Name);

    public override int GetHashCode() => Seq.Hash(Name);
}

/// <summary>PROPERTY (DDS Security 1.2, 7.2.5) of a participant: the dds.sec.*
/// properties turn on DDS Security (docs/security.md);
/// rustorb.dds.persistence.directory is where PERSISTENT writers keep their
/// files.</summary>
public sealed record Property(ImmutableDictionary<string, string> Value)
{
    public static readonly Property Empty = new(ImmutableDictionary<string, string>.Empty);

    public Property With(string name, string value) => new(Value.SetItem(name, value));

    public bool Equals(Property? other) =>
        other is not null && Value.Count == other.Value.Count
        && Value.All(kv => other.Value.TryGetValue(kv.Key, out string? v) && v == kv.Value);

    public override int GetHashCode()
    {
        int h = 0;
        foreach (var kv in Value)
        {
            h ^= HashCode.Combine(kv.Key, kv.Value);
        }
        return h;
    }
}

/// <summary>Equality of the lists of the policies by their elements (an
/// ImmutableArray compares its array).</summary>
internal static class Seq
{
    internal static bool Equal<T>(ImmutableArray<T> a, ImmutableArray<T> b) =>
        (a.IsDefaultOrEmpty && b.IsDefaultOrEmpty) || (!a.IsDefault && !b.IsDefault && a.AsSpan().SequenceEqual(b.AsSpan(), null));

    internal static int Hash<T>(ImmutableArray<T> a)
    {
        var h = new HashCode();
        if (!a.IsDefault)
        {
            foreach (T e in a)
            {
                h.Add(e);
            }
        }
        return h.ToHashCode();
    }
}

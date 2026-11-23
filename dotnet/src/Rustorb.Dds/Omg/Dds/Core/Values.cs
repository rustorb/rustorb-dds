// Duration, Time and InstanceHandle (DDS 1.4, 2.3.3 / 2.2.2.1).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using Rustorb.Dds.Native;

namespace Omg.Dds.Core;

/// <summary>A Duration_t: seconds and nanoseconds; Infinite is DURATION_INFINITE.</summary>
public readonly record struct Duration(int Sec, uint Nanosec)
{
    public static readonly Duration Infinite = new(0x7fffffff, 0x7fffffffu);
    public static readonly Duration Zero = new(0, 0);

    public bool IsInfinite => Sec == 0x7fffffff && Nanosec == 0x7fffffffu;

    public static Duration FromSeconds(double seconds) => FromTimeSpan(TimeSpan.FromSeconds(seconds));

    public static Duration FromMilliseconds(long ms) => new((int)(ms / 1000), (uint)(ms % 1000) * 1_000_000u);

    /// <summary>A TimeSpan as a Duration; TimeSpan.MaxValue and
    /// Timeout.InfiniteTimeSpan are Infinite.</summary>
    public static Duration FromTimeSpan(TimeSpan t)
    {
        if (t == TimeSpan.MaxValue || t == System.Threading.Timeout.InfiniteTimeSpan)
        {
            return Infinite;
        }
        if (t < TimeSpan.Zero)
        {
            throw new ArgumentOutOfRangeException(nameof(t), "a negative duration");
        }
        long ticks = t.Ticks;
        long sec = ticks / TimeSpan.TicksPerSecond;
        if (sec >= 0x7fffffff)
        {
            return Infinite;
        }
        return new((int)sec, (uint)(ticks % TimeSpan.TicksPerSecond) * 100u);
    }

    public TimeSpan ToTimeSpan() =>
        IsInfinite ? System.Threading.Timeout.InfiniteTimeSpan
                   : TimeSpan.FromTicks(Sec * TimeSpan.TicksPerSecond + Nanosec / 100);

    public static implicit operator Duration(TimeSpan t) => FromTimeSpan(t);

    internal RddsDuration Native => new() { sec = Sec, nanosec = Nanosec };

    internal static Duration Of(RddsDuration d) => new(d.sec, d.nanosec);
}

/// <summary>A Time_t since 1970-01-01T00:00:00Z; Invalid is TIME_INVALID.</summary>
public readonly record struct Time(int Sec, uint Nanosec)
{
    public static readonly Time Invalid = new(-1, 0xffffffffu);

    public bool IsValid => !(Sec == -1 && Nanosec == 0xffffffffu);

    public static Time Now() => FromDateTimeOffset(DateTimeOffset.UtcNow);

    public static Time FromDateTimeOffset(DateTimeOffset t)
    {
        long ticks = t.UtcTicks - DateTimeOffset.UnixEpoch.UtcTicks;
        long sec = Math.DivRem(ticks, TimeSpan.TicksPerSecond, out long rest);
        if (rest < 0)
        {
            sec--;
            rest += TimeSpan.TicksPerSecond;
        }
        return new(checked((int)sec), (uint)rest * 100u);
    }

    public DateTimeOffset ToDateTimeOffset() =>
        DateTimeOffset.UnixEpoch.AddTicks(Sec * TimeSpan.TicksPerSecond + Nanosec / 100);

    internal RddsTime Native => new() { sec = Sec, nanosec = Nanosec };

    internal static Time Of(RddsTime t) => new(t.sec, t.nanosec);
}

/// <summary>An InstanceHandle_t: 16 octets (an instance's key hash, or an
/// entity's GUID); Nil is HANDLE_NIL.</summary>
public readonly struct InstanceHandle : IEquatable<InstanceHandle>
{
    private readonly byte[]? value;

    public InstanceHandle(ReadOnlySpan<byte> value)
    {
        if (value.Length != 16)
        {
            throw new ArgumentException("an InstanceHandle has 16 octets");
        }
        this.value = value.ToArray();
    }

    public static readonly InstanceHandle Nil = default;

    /// <summary>The 16 octets.</summary>
    public ReadOnlySpan<byte> Value => value ?? new byte[16];

    public bool IsNil
    {
        get
        {
            if (value is null)
            {
                return true;
            }
            foreach (byte b in value)
            {
                if (b != 0)
                {
                    return false;
                }
            }
            return true;
        }
    }

    public bool Equals(InstanceHandle other) => Value.SequenceEqual(other.Value);

    public override bool Equals(object? obj) => obj is InstanceHandle h && Equals(h);

    public override int GetHashCode()
    {
        var h = new HashCode();
        h.AddBytes(Value);
        return h.ToHashCode();
    }

    public static bool operator ==(InstanceHandle a, InstanceHandle b) => a.Equals(b);

    public static bool operator !=(InstanceHandle a, InstanceHandle b) => !a.Equals(b);

    public override string ToString() => Convert.ToHexString(Value).ToLowerInvariant();

    internal static unsafe InstanceHandle Of(RddsInstanceHandle h) => new(new ReadOnlySpan<byte>(h.value, 16));

    internal static unsafe InstanceHandle Of(RddsGuid g) => new(new ReadOnlySpan<byte>(g.value, 16));

    internal unsafe RddsInstanceHandle Native
    {
        get
        {
            var h = new RddsInstanceHandle();
            ReadOnlySpan<byte> v = Value;
            for (int i = 0; i < 16; i++)
            {
                h.value[i] = v[i];
            }
            return h;
        }
    }
}

// Conditions and WaitSets (DDS 1.4, 2.2.2.1.6 - 2.2.2.1.9).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections.Generic;
using Omg.Dds.Core.Status;
using Omg.Dds.Sub;
using Rustorb.Dds.Native;

namespace Omg.Dds.Core;

/// <summary>A condition a WaitSet waits for.</summary>
public abstract class Condition
{
    internal readonly ConditionHandle handle;

    internal Condition(ConditionHandle handle)
    {
        this.handle = handle;
    }

    internal nint Raw => handle.Raw;

    public bool TriggerValue
    {
        get
        {
            bool v = Ffi.rdds_condition_trigger_value(Raw) != 0;
            GC.KeepAlive(this);
            return v;
        }
    }
}

/// <summary>A condition whose trigger value the application sets.</summary>
public sealed unsafe class GuardCondition : Condition, IDisposable
{
    public GuardCondition() : base(Make())
    {
    }

    private static ConditionHandle Make()
    {
        Library.Check();
        nint c;
        Errors.Check(Ffi.rdds_guard_condition_new(&c));
        return new ConditionHandle(c);
    }

    public new bool TriggerValue
    {
        get => base.TriggerValue;
        set
        {
            Errors.Check(Ffi.rdds_guard_condition_set_trigger_value(Raw, (byte)(value ? 1 : 0)));
            GC.KeepAlive(this);
        }
    }

    public void Dispose() => handle.Dispose();
}

/// <summary>The condition of a writer's or reader's enabled statuses.</summary>
public sealed unsafe class StatusCondition : Condition
{
    internal StatusCondition(ConditionHandle handle, Entity entity) : base(handle)
    {
        Entity = entity;
    }

    public Entity Entity { get; }

    public StatusKind EnabledStatuses
    {
        get
        {
            uint m;
            Errors.Check(Ffi.rdds_status_condition_get_enabled_statuses(Raw, &m));
            GC.KeepAlive(this);
            return (StatusKind)m;
        }
        set
        {
            Errors.Check(Ffi.rdds_status_condition_set_enabled_statuses(Raw, (uint)value));
            GC.KeepAlive(this);
        }
    }
}

/// <summary>A condition true while a reader has samples in some states.</summary>
public sealed class ReadCondition : Condition, IDisposable
{
    internal ReadCondition(ConditionHandle handle, IDataReader reader, SampleState s, ViewState v, InstanceState i)
        : base(handle)
    {
        DataReader = reader;
        SampleStates = s;
        ViewStates = v;
        InstanceStates = i;
    }

    public IDataReader DataReader { get; }
    public SampleState SampleStates { get; }
    public ViewState ViewStates { get; }
    public InstanceState InstanceStates { get; }

    public void Dispose() => handle.Dispose();
}

/// <summary>Waits until one of its conditions is true.</summary>
public sealed unsafe class WaitSet : IDisposable
{
    private readonly WaitSetHandle handle;
    private readonly List<Condition> conditions = new();

    public WaitSet()
    {
        Library.Check();
        nint w;
        Errors.Check(Ffi.rdds_waitset_new(&w));
        handle = new WaitSetHandle(w);
    }

    private nint Raw => handle.Raw;

    public IReadOnlyList<Condition> Conditions
    {
        get
        {
            lock (conditions)
            {
                return conditions.ToArray();
            }
        }
    }

    public void AttachCondition(Condition c)
    {
        lock (conditions)
        {
            Errors.Check(Ffi.rdds_waitset_attach_condition(Raw, c.Raw));
            GC.KeepAlive(c);
            conditions.Add(c);
        }
    }

    public void DetachCondition(Condition c)
    {
        lock (conditions)
        {
            Errors.Check(Ffi.rdds_waitset_detach_condition(Raw, c.Raw));
            GC.KeepAlive(c);
            conditions.Remove(c);
        }
    }

    /// <summary>The conditions that are true, once one is; TimeoutException.</summary>
    public IList<Condition> Wait(Duration timeout)
    {
        Condition[] attached;
        lock (conditions)
        {
            attached = conditions.ToArray();
        }
        var active = new nuint[Math.Max(attached.Length, 1)];
        nuint n;
        fixed (nuint* a = active)
        {
            Errors.Check(Ffi.rdds_waitset_wait(Raw, timeout.Native, a, (nuint)active.Length, &n));
        }
        GC.KeepAlive(this);
        var r = new List<Condition>();
        for (int i = 0; i < (int)Math.Min(n, (nuint)active.Length); i++)
        {
            int k = (int)active[i];
            if (k < attached.Length)
            {
                r.Add(attached[k]);
            }
        }
        return r;
    }

    public void Dispose() => handle.Dispose();
}

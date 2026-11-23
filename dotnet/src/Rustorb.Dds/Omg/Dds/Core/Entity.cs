// The base of the entities, and the helpers they share.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Threading;

namespace Omg.Dds.Core
{
    /// <summary>An entity (DDS 1.4, 2.2.2.1.2). Dispose deletes it, once.</summary>
    public abstract class Entity : IDisposable
    {
        private int closed;

        /// <summary>The entity's GUID, or its handle.</summary>
        public abstract InstanceHandle InstanceHandle { get; }

        public bool IsClosed => Volatile.Read(ref closed) != 0;

        public void Dispose()
        {
            if (Interlocked.Exchange(ref closed, 1) == 0)
            {
                Close();
            }
            GC.SuppressFinalize(this);
        }

        protected abstract void Close();
    }
}

namespace Rustorb.Dds.Native
{
    /// <summary>The writers and readers by their GUID, for the listeners of
    /// their publishers, subscribers and participants.</summary>
    internal static class Endpoints
    {
        private static readonly object gate = new();
        private static readonly Dictionary<Omg.Dds.Core.InstanceHandle, WeakReference<object>> all = new();

        // The statuses of endpoints not known yet: those of a writer or a
        // reader that come while it is being made, before its GUID is
        // known here. Kept a while for Add, then dropped.
        private static readonly Dictionary<Omg.Dds.Core.InstanceHandle, (long Since, List<Action<object>> Calls)> pending = new();

        private const long KeepMs = 10_000;

        internal static void Add(Omg.Dds.Core.InstanceHandle guid, object e)
        {
            List<Action<object>>? calls = null;
            lock (gate)
            {
                all[guid] = new WeakReference<object>(e);
                if (pending.Remove(guid, out var p))
                {
                    calls = p.Calls;
                }
            }
            if (calls != null)
            {
                foreach (Action<object> a in calls)
                {
                    try
                    {
                        a(e);
                    }
                    catch (Exception ex)
                    {
                        System.Diagnostics.Trace.TraceError("rustorb-dds: a listener threw {0}", ex);
                    }
                }
            }
        }

        internal static void Remove(Omg.Dds.Core.InstanceHandle guid)
        {
            lock (gate)
            {
                all.Remove(guid);
            }
        }

        internal static object? Find(Omg.Dds.Core.InstanceHandle guid)
        {
            lock (gate)
            {
                return all.TryGetValue(guid, out WeakReference<object>? w) && w.TryGetTarget(out object? e) ? e : null;
            }
        }

        /// <summary>Calls a with the endpoint of guid, now or when it is added.</summary>
        internal static void Deliver(Omg.Dds.Core.InstanceHandle guid, Action<object> a)
        {
            object? e = null;
            lock (gate)
            {
                if (all.TryGetValue(guid, out WeakReference<object>? w))
                {
                    if (!w.TryGetTarget(out e))
                    {
                        return;
                    }
                }
                else
                {
                    long now = Environment.TickCount64;
                    List<Omg.Dds.Core.InstanceHandle>? old = null;
                    foreach (var kv in pending)
                    {
                        if (now - kv.Value.Since > KeepMs)
                        {
                            (old ??= new()).Add(kv.Key);
                        }
                    }
                    old?.ForEach(k => pending.Remove(k));
                    if (!pending.TryGetValue(guid, out var p))
                    {
                        p = (now, new List<Action<object>>());
                        pending[guid] = p;
                    }
                    p.Calls.Add(a);
                    return;
                }
            }
            a(e);
        }
    }

    /// <summary>An array of NUL-terminated UTF-8 strings, pinned until disposed.</summary>
    internal sealed unsafe class Utf8Array : IDisposable
    {
        private readonly List<GCHandle> pins = new();
        private GCHandle array;

        internal Utf8Array(IReadOnlyList<string> strings)
        {
            var ptrs = new nint[strings.Count];
            for (int i = 0; i < ptrs.Length; i++)
            {
                GCHandle h = GCHandle.Alloc(Utf8.Z(strings[i]), GCHandleType.Pinned);
                pins.Add(h);
                ptrs[i] = h.AddrOfPinnedObject();
            }
            array = GCHandle.Alloc(ptrs, GCHandleType.Pinned);
        }

        internal byte** Pointer => (byte**)array.AddrOfPinnedObject();

        public void Dispose()
        {
            foreach (GCHandle h in pins)
            {
                h.Free();
            }
            pins.Clear();
            if (array.IsAllocated)
            {
                array.Free();
            }
        }
    }
}

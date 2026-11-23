// Helpers of the classes rustorb-idl writes for -bcsharp -Wbdds_only:
// arrays filled with default values, and equality and hashes of sequences
// and arrays by their elements.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections;
using System.Collections.Generic;

namespace Rustorb.Dds;

/// <summary>Helpers of generated code.</summary>
public static class Values
{
    /// <summary>An array of n values made by f.</summary>
    public static T[] Array<T>(int n, Func<T> f)
    {
        var a = new T[n];
        for (int i = 0; i < n; i++)
        {
            a[i] = f();
        }
        return a;
    }

    /// <summary>Whether two sequences or arrays have equal elements (nested
    /// ones compared the same way).</summary>
    public static bool Equal(IEnumerable? a, IEnumerable? b)
    {
        if (ReferenceEquals(a, b))
        {
            return true;
        }
        if (a is null || b is null)
        {
            return false;
        }
        IEnumerator x = a.GetEnumerator();
        IEnumerator y = b.GetEnumerator();
        while (true)
        {
            bool mx = x.MoveNext();
            bool my = y.MoveNext();
            if (mx != my)
            {
                return false;
            }
            if (!mx)
            {
                return true;
            }
            if (!Element(x.Current, y.Current))
            {
                return false;
            }
        }
    }

    /// <summary>The hash of the elements of a sequence or array.</summary>
    public static int Hash(IEnumerable? a)
    {
        if (a is null)
        {
            return 0;
        }
        var h = new HashCode();
        foreach (object? e in a)
        {
            h.Add(e is IEnumerable n && e is not string ? Hash(n) : EqualityComparer<object?>.Default.GetHashCode(e));
        }
        return h.ToHashCode();
    }

    private static bool Element(object? a, object? b)
    {
        if (a is IEnumerable x && a is not string && b is IEnumerable y && b is not string)
        {
            return Equal(x, y);
        }
        return Equals(a, b);
    }
}

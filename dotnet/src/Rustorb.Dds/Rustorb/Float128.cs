// long double (IEEE 754 binary128) and decimal, as the IDL4 to C# mapping
// has long double: rounded to nearest, ties to even, both ways.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Numerics;

namespace Rustorb.Dds;

/// <summary>binary128 bits and decimal.</summary>
public static class Float128
{
    private const int Bias = 16383;
    private const int Mantissa = 112;

    /// <summary>The binary128 bits of a decimal (exact for 0, else rounded
    /// to nearest even).</summary>
    public static (ulong High, ulong Low) FromDecimal(decimal v)
    {
        int[] bits = decimal.GetBits(v);
        bool negative = (bits[3] & unchecked((int)0x80000000)) != 0;
        int scale = (bits[3] >> 16) & 0xff;
        BigInteger m = new BigInteger((uint)bits[0]) | (new BigInteger((uint)bits[1]) << 32)
            | (new BigInteger((uint)bits[2]) << 64);
        ulong sign = negative ? 1UL << 63 : 0;
        if (m.IsZero)
        {
            return (sign, 0);
        }
        BigInteger den = BigInteger.Pow(10, scale);
        // value = m / den; find e with 2^112 <= value * 2^-e < 2^113.
        long e = (long)(m.GetBitLength() - den.GetBitLength());
        BigInteger q;
        while (true)
        {
            q = Scaled(m, den, e - Mantissa);
            if (q < (BigInteger.One << Mantissa))
            {
                e--;
                continue;
            }
            if (q >= (BigInteger.One << (Mantissa + 1)))
            {
                e++;
                continue;
            }
            break;
        }
        // Rounded: it may carry into 2^113.
        if (q == (BigInteger.One << (Mantissa + 1)))
        {
            q >>= 1;
            e++;
        }
        long biased = e + Bias;
        // A decimal is within 1e-28 .. 8e28, far from the subnormals and
        // infinities of binary128.
        BigInteger frac = q - (BigInteger.One << Mantissa);
        ulong low = (ulong)(frac & ulong.MaxValue);
        ulong high = (ulong)(frac >> 64) | ((ulong)biased << 48) | sign;
        return (high, low);
    }

    /// <summary>round(m / den * 2^-shift), ties to even.</summary>
    private static BigInteger Scaled(BigInteger m, BigInteger den, long shift)
    {
        BigInteger num = m, d = den;
        if (shift >= 0)
        {
            d <<= (int)shift;
        }
        else
        {
            num <<= (int)-shift;
        }
        BigInteger q = BigInteger.DivRem(num, d, out BigInteger r);
        BigInteger twice = r << 1;
        int c = twice.CompareTo(d);
        if (c > 0 || (c == 0 && !q.IsEven))
        {
            q += 1;
        }
        return q;
    }

    /// <summary>The decimal of binary128 bits, rounded to nearest even;
    /// OverflowException for values out of decimal's range, infinities and
    /// NaNs.</summary>
    public static decimal ToDecimal(ulong high, ulong low)
    {
        bool negative = (high >> 63) != 0;
        int biased = (int)((high >> 48) & 0x7fff);
        BigInteger frac = (new BigInteger(high & 0xffff_ffff_ffffUL) << 64) | low;
        if (biased == 0x7fff)
        {
            throw new OverflowException("a long double infinity or NaN has no decimal");
        }
        BigInteger sig;
        long e;
        if (biased == 0)
        {
            if (frac.IsZero)
            {
                return negative ? -0.0m : 0.0m;
            }
            sig = frac;
            e = 1 - Bias - Mantissa;
        }
        else
        {
            sig = frac | (BigInteger.One << Mantissa);
            e = biased - Bias - Mantissa;
        }
        // value = sig * 2^e. The largest scale (<= 28) whose rounded
        // integer fits in 96 bits.
        BigInteger limit = BigInteger.One << 96;
        for (int scale = 28; scale >= 0; scale--)
        {
            BigInteger num = sig * BigInteger.Pow(10, scale);
            BigInteger q;
            if (e >= 0)
            {
                q = num << (int)e;
            }
            else
            {
                BigInteger d = BigInteger.One << (int)-e;
                q = BigInteger.DivRem(num, d, out BigInteger r);
                int c = (r << 1).CompareTo(d);
                if (c > 0 || (c == 0 && !q.IsEven))
                {
                    q += 1;
                }
            }
            if (q < limit)
            {
                if (q.IsZero)
                {
                    return 0.0m;
                }
                int lo = (int)(uint)(q & uint.MaxValue);
                int mid = (int)(uint)((q >> 32) & uint.MaxValue);
                int hi = (int)(uint)((q >> 64) & uint.MaxValue);
                return new decimal(lo, mid, hi, negative, (byte)scale);
            }
        }
        throw new OverflowException("a long double out of the range of decimal");
    }
}

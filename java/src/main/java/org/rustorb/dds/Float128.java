// The Java mapping of long double is double; samples carry IEEE 754
// binary128.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

/** Converts between double and the bits of IEEE 754 binary128. */
public final class Float128 {
    private Float128() {}

    /** The high 64 bits of the binary128 equal to d (exactly). */
    public static long high(double d) {
        long b = Double.doubleToRawLongBits(d);
        long sign = b & 0x8000000000000000L;
        int e = (int) ((b >>> 52) & 0x7ff);
        long f = b & 0xFFFFFFFFFFFFFL;
        if (e == 0x7ff) {
            return sign | (0x7fffL << 48) | (f >>> 4);
        }
        if (e == 0) {
            if (f == 0) {
                return sign;
            }
            int shift = Long.numberOfLeadingZeros(f) - 11;
            f = (f << shift) & 0xFFFFFFFFFFFFFL;
            e = 1 - shift;
        }
        long e128 = e - 1023 + 16383;
        return sign | (e128 << 48) | (f >>> 4);
    }

    /** The low 64 bits of the binary128 equal to d. */
    public static long low(double d) {
        long b = Double.doubleToRawLongBits(d);
        int e = (int) ((b >>> 52) & 0x7ff);
        long f = b & 0xFFFFFFFFFFFFFL;
        if (e == 0 && f != 0) {
            int shift = Long.numberOfLeadingZeros(f) - 11;
            f = (f << shift) & 0xFFFFFFFFFFFFFL;
        }
        return f << 60;
    }

    /** The binary128 of the bits as a double, rounded to nearest. */
    public static double toDouble(long high, long low) {
        boolean negative = high < 0;
        int e = (int) ((high >>> 48) & 0x7fff);
        long fhi = high & 0xFFFFFFFFFFFFL;
        if (e == 0x7fff) {
            if ((fhi | low) != 0) {
                return Double.NaN;
            }
            return negative ? Double.NEGATIVE_INFINITY : Double.POSITIVE_INFINITY;
        }
        if (e == 0) {
            // Zero, or a subnormal far below the range of double.
            return negative ? -0.0 : 0.0;
        }
        // The 112 bits of the fraction, cut to 62 with what is cut off as a
        // sticky bit, so that the conversion to double rounds correctly.
        long top = (fhi << 14) | (low >>> 50);
        long sticky = (low & ((1L << 50) - 1)) != 0 ? 1 : 0;
        long m = (1L << 62) | top | sticky;
        double r = Math.scalb((double) m, e - 16383 - 62);
        return negative ? -r : r;
    }
}

//! AES-GCM and AES-GMAC (NIST SP 800-38D) for the builtin cryptographic
//! plugin, with a GHASH that multiplies eight blocks at a time on x86-64
//! (rustorb-dds#119).
//!
//! RustCrypto's GHASH (polyval 0.6) multiplies and reduces one block at a
//! time, about 1.5 GB/s, and GHASH is most of what signing or encrypting
//! a large sample costs. Here, with PCLMULQDQ, the products of eight
//! blocks by H^8 .. H^1 are summed unreduced and reduced once (the
//! reduction is linear), which is several times faster. Elsewhere (no
//! PCLMULQDQ, other architectures) GHASH is RustCrypto's. AES itself is
//! RustCrypto's `aes` (AES-NI where there is one) in counter mode.
//!
//! GHASH is computed as POLYVAL (RFC 8452, appendix A): GHASH(H, X) is
//! the byte reversal of POLYVAL(mulX_POLYVAL(rev(H)), rev(X1), ...), so
//! that a block loaded into a register needs no bit reflection.
//!
//! Tested against RustCrypto's `aes-gcm` on random keys, nonces and
//! lengths, and against the test vectors of the GCM specification.

use aes::cipher::{BlockEncrypt, KeyInit};
use aes::{Aes128, Aes256, Block};

/// An AES-GCM key: AES-128 or AES-256, with its GHASH key.
#[derive(Clone)]
pub(crate) struct Gcm {
    aes: AesKey,
    ghash: GhashKey,
}

impl std::fmt::Debug for Gcm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Gcm")
    }
}

// Gcm itself is boxed by its users.
#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
enum AesKey {
    A128(Aes128),
    A256(Aes256),
}

impl AesKey {
    fn encrypt_blocks(&self, blocks: &mut [Block]) {
        match self {
            AesKey::A128(k) => k.encrypt_blocks(blocks),
            AesKey::A256(k) => k.encrypt_blocks(blocks),
        }
    }
}

/// A GCM tag.
pub(crate) type Tag = [u8; 16];

/// The tag does not verify.
#[derive(Debug)]
pub(crate) struct BadTag;

impl Gcm {
    /// From a 16-octet (AES-128) or 32-octet (AES-256) key.
    pub(crate) fn new(key: &[u8]) -> Gcm {
        let aes = if key.len() == 16 {
            AesKey::A128(Aes128::new_from_slice(key).expect("16 octets"))
        } else {
            AesKey::A256(Aes256::new_from_slice(key).expect("32 octets"))
        };
        let mut h = [Block::default()];
        aes.encrypt_blocks(&mut h);
        let h: [u8; 16] = h[0].into();
        Gcm {
            aes,
            ghash: GhashKey::new(&h),
        }
    }

    /// Encrypts `buf` in place, authenticating `aad` too: the tag.
    pub(crate) fn seal(&self, iv: &[u8; 12], aad: &[u8], buf: &mut [u8]) -> Tag {
        self.ctr(iv, buf);
        self.tag(iv, aad, buf)
    }

    /// Checks `tag` over `aad` and `buf` (the ciphertext), then decrypts
    /// `buf` in place. `buf` is left as it is if the tag does not verify.
    pub(crate) fn open(
        &self,
        iv: &[u8; 12],
        aad: &[u8],
        buf: &mut [u8],
        tag: &Tag,
    ) -> Result<(), BadTag> {
        let t = self.tag(iv, aad, buf);
        // In constant time.
        if t.iter().zip(tag).fold(0u8, |d, (a, b)| d | (a ^ b)) != 0 {
            return Err(BadTag);
        }
        self.ctr(iv, buf);
        Ok(())
    }

    /// AES-GMAC: the tag of `data`, authenticated only.
    pub(crate) fn mac(&self, iv: &[u8; 12], data: &[u8]) -> Tag {
        self.tag(iv, data, &[])
    }

    /// E(K, J0) xor GHASH(H, A, C, len(A) || len(C)), J0 = IV || 1.
    fn tag(&self, iv: &[u8; 12], aad: &[u8], ciphertext: &[u8]) -> Tag {
        let mut lens = [0u8; 16];
        lens[..8].copy_from_slice(&((aad.len() as u64) * 8).to_be_bytes());
        lens[8..].copy_from_slice(&((ciphertext.len() as u64) * 8).to_be_bytes());
        let s = self.ghash.hash(&[aad, ciphertext], &lens);
        let mut j0 = [counter_block(iv, 1)];
        self.aes.encrypt_blocks(&mut j0);
        let mut t = [0u8; 16];
        for (i, o) in t.iter_mut().enumerate() {
            *o = j0[0][i] ^ s[i];
        }
        t
    }

    /// Counter mode from inc32(J0): what GCM encrypts and decrypts with
    /// (RustCrypto's `ctr`, as `aes-gcm` uses it).
    fn ctr(&self, iv: &[u8; 12], buf: &mut [u8]) {
        use aes::cipher::{InnerIvInit, StreamCipherCore};
        type Ctr32Be<C> = ctr::CtrCore<C, ctr::flavors::Ctr32BE>;
        let iv = counter_block(iv, 2);
        match &self.aes {
            AesKey::A128(k) => Ctr32Be::inner_iv_init(k, &iv).apply_keystream_partial(buf.into()),
            AesKey::A256(k) => Ctr32Be::inner_iv_init(k, &iv).apply_keystream_partial(buf.into()),
        }
    }
}

fn counter_block(iv: &[u8; 12], counter: u32) -> Block {
    let mut b = Block::default();
    b[..12].copy_from_slice(iv);
    b[12..].copy_from_slice(&counter.to_be_bytes());
    b
}

/// The GHASH key H, with what the fast path precomputed.
#[derive(Clone)]
enum GhashKey {
    #[cfg(target_arch = "x86_64")]
    Clmul(Box<clmul::Powers>),
    Soft([u8; 16]),
}

impl GhashKey {
    fn new(h: &[u8; 16]) -> GhashKey {
        #[cfg(target_arch = "x86_64")]
        if clmul::available() {
            return GhashKey::Clmul(Box::new(clmul::Powers::new(h)));
        }
        GhashKey::Soft(*h)
    }

    /// GHASH of each of `parts` padded with zeros to whole blocks, then
    /// of `last` (the lengths block).
    fn hash(&self, parts: &[&[u8]], last: &[u8; 16]) -> [u8; 16] {
        match self {
            #[cfg(target_arch = "x86_64")]
            GhashKey::Clmul(p) => p.hash(parts, last),
            GhashKey::Soft(h) => soft_hash(h, parts, last),
        }
    }
}

/// RustCrypto's GHASH.
fn soft_hash(h: &[u8; 16], parts: &[&[u8]], last: &[u8; 16]) -> [u8; 16] {
    use ghash::universal_hash::UniversalHash;
    let mut g = <ghash::GHash as ghash::universal_hash::KeyInit>::new(h.into());
    for p in parts {
        g.update_padded(p);
    }
    g.update(&[(*last).into()]);
    g.finalize().into()
}

/// mulX_POLYVAL (RFC 8452, appendix A) of a block read little-endian.
#[cfg(target_arch = "x86_64")]
fn mulx(b: &[u8; 16]) -> [u8; 16] {
    let v = u128::from_le_bytes(*b);
    let hi = v >> 127;
    let v = (v << 1) ^ hi ^ (hi << 127) ^ (hi << 126) ^ (hi << 121);
    v.to_le_bytes()
}

#[cfg(target_arch = "x86_64")]
mod clmul {
    //! POLYVAL with PCLMULQDQ: dot(a, b) = a * b * x^-128 in
    //! GF(2)[x] / (x^128 + x^127 + x^126 + x^121 + 1). Eight blocks:
    //! dot(y ^ x1, H^8) ^ dot(x2, H^7) ^ ... ^ dot(x8, H^1), with H^k the
    //! k-th dot power, the Karatsuba products summed before the one
    //! reduction.

    use core::arch::x86_64::*;

    pub(super) fn available() -> bool {
        std::is_x86_feature_detected!("pclmulqdq") && std::is_x86_feature_detected!("ssse3")
    }

    /// H^1 .. H^8 (POLYVAL key powers), and for each the xor of its two
    /// halves (for Karatsuba), as octets.
    #[derive(Clone)]
    pub(super) struct Powers {
        h: [[u8; 16]; 8],
        k: [[u8; 16]; 8],
    }

    impl Powers {
        pub(super) fn new(ghash_h: &[u8; 16]) -> Powers {
            let mut rev = *ghash_h;
            rev.reverse();
            let h1 = super::mulx(&rev);
            // SAFETY: `available` was checked by the caller.
            unsafe { Self::compute(&h1) }
        }

        #[target_feature(enable = "pclmulqdq,ssse3,sse2")]
        unsafe fn compute(h1: &[u8; 16]) -> Powers {
            let mut h = [[0u8; 16]; 8];
            let mut k = [[0u8; 16]; 8];
            // SAFETY: unaligned loads and stores of 16 octets.
            unsafe {
                let base = _mm_loadu_si128(h1.as_ptr().cast());
                let mut p = base;
                for i in 0..8 {
                    _mm_storeu_si128(h[i].as_mut_ptr().cast(), p);
                    _mm_storeu_si128(k[i].as_mut_ptr().cast(), halves(p));
                    let (a, b, c) = product(p, base, halves(base));
                    p = reduce(a, b, c);
                }
            }
            Powers { h, k }
        }

        pub(super) fn hash(&self, parts: &[&[u8]], last: &[u8; 16]) -> [u8; 16] {
            // SAFETY: Powers exist only where `available` held.
            unsafe { self.hash_clmul(parts, last) }
        }

        #[target_feature(enable = "pclmulqdq,ssse3,sse2")]
        unsafe fn hash_clmul(&self, parts: &[&[u8]], last: &[u8; 16]) -> [u8; 16] {
            // SAFETY: unaligned loads of 16 octets within the slices,
            // and of the local arrays.
            unsafe {
                let rev = _mm_set_epi8(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
                let load = |p: *const u8| _mm_shuffle_epi8(_mm_loadu_si128(p.cast()), rev);
                let mut hs = [_mm_setzero_si128(); 8];
                let mut ks = [_mm_setzero_si128(); 8];
                for i in 0..8 {
                    hs[i] = _mm_loadu_si128(self.h[i].as_ptr().cast());
                    ks[i] = _mm_loadu_si128(self.k[i].as_ptr().cast());
                }
                let mut y = _mm_setzero_si128();
                let one = |y: __m128i, x: __m128i| {
                    let (a, b, c) = product(_mm_xor_si128(y, x), hs[0], ks[0]);
                    reduce(a, b, c)
                };
                for part in parts {
                    let mut chunks = part.chunks_exact(128);
                    for c in &mut chunks {
                        let p = c.as_ptr();
                        let x0 = _mm_xor_si128(y, load(p));
                        let (mut a, mut b, mut m) = product(x0, hs[7], ks[7]);
                        for j in 1..8 {
                            let (a2, b2, m2) = product(load(p.add(16 * j)), hs[7 - j], ks[7 - j]);
                            a = _mm_xor_si128(a, a2);
                            b = _mm_xor_si128(b, b2);
                            m = _mm_xor_si128(m, m2);
                        }
                        y = reduce(a, b, m);
                    }
                    let rest = chunks.remainder();
                    let mut blocks = rest.chunks_exact(16);
                    for c in &mut blocks {
                        y = one(y, load(c.as_ptr()));
                    }
                    let tail = blocks.remainder();
                    if !tail.is_empty() {
                        let mut b = [0u8; 16];
                        b[..tail.len()].copy_from_slice(tail);
                        y = one(y, load(b.as_ptr()));
                    }
                }
                y = one(y, load(last.as_ptr()));
                let mut out = [0u8; 16];
                _mm_storeu_si128(out.as_mut_ptr().cast(), _mm_shuffle_epi8(y, rev));
                out
            }
        }
    }

    /// The low half of `v` xor its high half, in the low lane.
    #[inline(always)]
    unsafe fn halves(v: __m128i) -> __m128i {
        // SAFETY: register operations of SSE2.
        unsafe { _mm_xor_si128(v, _mm_shuffle_epi32(v, 0x0E)) }
    }

    /// The three Karatsuba products of `a` by `h` (`hk`: the halves of
    /// `h` xored): lo*lo, hi*hi, (lo^hi)*(lo^hi). Sums of these are
    /// reduced by [`reduce`].
    #[inline(always)]
    unsafe fn product(a: __m128i, h: __m128i, hk: __m128i) -> (__m128i, __m128i, __m128i) {
        // SAFETY: PCLMULQDQ and SSE2 register operations, enabled by the
        // callers' target features.
        unsafe {
            (
                _mm_clmulepi64_si128(a, h, 0x00),
                _mm_clmulepi64_si128(a, h, 0x11),
                _mm_clmulepi64_si128(halves(a), hk, 0x00),
            )
        }
    }

    /// The 256-bit product from the Karatsuba terms, times x^-128,
    /// reduced (as RustCrypto's polyval does for one block).
    #[inline(always)]
    unsafe fn reduce(t0: __m128i, t1: __m128i, t2: __m128i) -> __m128i {
        // SAFETY: SSE2 register operations.
        unsafe {
            let t2 = _mm_xor_si128(t2, _mm_xor_si128(t0, t1));
            let v0 = t0;
            let v1 = _mm_xor_si128(_mm_shuffle_epi32(t0, 0x0E), t2);
            let v2 = _mm_xor_si128(t1, _mm_shuffle_epi32(t2, 0x0E));
            let v3 = _mm_shuffle_epi32(t1, 0x0E);
            let v2 = x5(
                v2,
                v0,
                _mm_srli_epi64(v0, 1),
                _mm_srli_epi64(v0, 2),
                _mm_srli_epi64(v0, 7),
            );
            let v1 = x4(
                v1,
                _mm_slli_epi64(v0, 63),
                _mm_slli_epi64(v0, 62),
                _mm_slli_epi64(v0, 57),
            );
            let v3 = x5(
                v3,
                v1,
                _mm_srli_epi64(v1, 1),
                _mm_srli_epi64(v1, 2),
                _mm_srli_epi64(v1, 7),
            );
            let v2 = x4(
                v2,
                _mm_slli_epi64(v1, 63),
                _mm_slli_epi64(v1, 62),
                _mm_slli_epi64(v1, 57),
            );
            _mm_unpacklo_epi64(v2, v3)
        }
    }

    #[inline(always)]
    unsafe fn x4(a: __m128i, b: __m128i, c: __m128i, d: __m128i) -> __m128i {
        // SAFETY: SSE2 register operations.
        unsafe { _mm_xor_si128(_mm_xor_si128(a, b), _mm_xor_si128(c, d)) }
    }

    #[inline(always)]
    unsafe fn x5(a: __m128i, b: __m128i, c: __m128i, d: __m128i, e: __m128i) -> __m128i {
        // SAFETY: SSE2 register operations.
        unsafe { _mm_xor_si128(a, x4(b, c, d, e)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::AeadInPlace;

    /// xorshift64, for test inputs.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn bytes(&mut self, n: usize) -> Vec<u8> {
            (0..n).map(|_| self.next() as u8).collect()
        }
    }

    fn reference(key: &[u8], iv: &[u8; 12], aad: &[u8], data: &[u8]) -> (Vec<u8>, [u8; 16]) {
        let mut buf = data.to_vec();
        let n = aes_gcm::Nonce::from(*iv);
        let t = if key.len() == 16 {
            aes_gcm::Aes128Gcm::new_from_slice(key)
                .unwrap()
                .encrypt_in_place_detached(&n, aad, &mut buf)
        } else {
            aes_gcm::Aes256Gcm::new_from_slice(key)
                .unwrap()
                .encrypt_in_place_detached(&n, aad, &mut buf)
        }
        .unwrap();
        (buf, t.into())
    }

    #[test]
    fn as_rustcrypto_on_random_inputs() {
        let mut r = Rng(0x9e37_79b9_7f4a_7c15);
        let mut lens: Vec<usize> = (0..300).collect();
        lens.extend([511, 512, 513, 1023, 1024, 1025, 4096, 4111, 65536, 65537]);
        for (i, &n) in lens.iter().enumerate() {
            for key_len in [16, 32] {
                let key = r.bytes(key_len);
                let iv: [u8; 12] = r.bytes(12).try_into().unwrap();
                let data = r.bytes(n);
                let aad = r.bytes((i * 7) % 300);
                let g = Gcm::new(&key);
                let (want, want_tag) = reference(&key, &iv, &aad, &data);
                let mut buf = data.clone();
                let tag = g.seal(&iv, &aad, &mut buf);
                assert_eq!(buf, want, "ciphertext, {n} octets");
                assert_eq!(tag, want_tag, "tag, {n} octets");
                g.open(&iv, &aad, &mut buf, &tag).unwrap();
                assert_eq!(buf, data);
                // GMAC: everything as additional data.
                let (_, want_mac) = reference(&key, &iv, &data, &[]);
                assert_eq!(g.mac(&iv, &data), want_mac, "GMAC, {n} octets");
            }
        }
    }

    #[test]
    fn the_fast_ghash_equals_rustcrypto() {
        let mut r = Rng(42);
        for n in [0, 1, 15, 16, 17, 127, 128, 129, 1000, 4096, 70000] {
            let h: [u8; 16] = r.bytes(16).try_into().unwrap();
            let a = r.bytes(n / 3);
            let b = r.bytes(n);
            let last: [u8; 16] = r.bytes(16).try_into().unwrap();
            let k = GhashKey::new(&h);
            assert_eq!(
                k.hash(&[&a, &b], &last),
                soft_hash(&h, &[&a, &b], &last),
                "{n} octets"
            );
        }
    }

    #[test]
    fn without_clmul_as_with_it() {
        let mut r = Rng(7);
        for n in [0, 33, 4096, 65536] {
            let key = r.bytes(32);
            let iv: [u8; 12] = r.bytes(12).try_into().unwrap();
            let data = r.bytes(n);
            let fast = Gcm::new(&key);
            let mut soft = fast.clone();
            let mut h = [Block::default()];
            soft.aes.encrypt_blocks(&mut h);
            soft.ghash = GhashKey::Soft(h[0].into());
            let (mut a, mut b) = (data.clone(), data.clone());
            assert_eq!(fast.seal(&iv, b"x", &mut a), soft.seal(&iv, b"x", &mut b));
            assert_eq!(a, b);
            assert_eq!(fast.mac(&iv, &data), soft.mac(&iv, &data));
        }
    }

    #[test]
    fn a_wrong_tag_is_refused_and_leaves_the_data() {
        let g = Gcm::new(&[1; 16]);
        let iv = [2; 12];
        let mut buf = vec![3u8; 100];
        let mut tag = g.seal(&iv, b"aad", &mut buf);
        let sealed = buf.clone();
        tag[0] ^= 1;
        assert!(g.open(&iv, b"aad", &mut buf, &tag).is_err());
        assert_eq!(buf, sealed);
        tag[0] ^= 1;
        assert!(g.open(&iv, b"aad!", &mut buf, &tag).is_err());
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn the_test_vectors_of_the_gcm_specification() {
        // McGrew and Viega, "The Galois/Counter Mode of Operation", test
        // cases 2, 4 (AES-128) and 16 (AES-256), 96-bit IVs.
        let cases = [
            (
                "00000000000000000000000000000000",
                "000000000000000000000000",
                "",
                "00000000000000000000000000000000",
                "0388dace60b6a392f328c2b971b2fe78",
                "ab6e47d42cec13bdf53a67b21257bddf",
            ),
            (
                "feffe9928665731c6d6a8f9467308308",
                "cafebabefacedbaddecaf888",
                "feedfacedeadbeeffeedfacedeadbeefabaddad2",
                "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
                "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091",
                "5bc94fbc3221a5db94fae95ae7121a47",
            ),
            (
                "feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308",
                "cafebabefacedbaddecaf888",
                "feedfacedeadbeeffeedfacedeadbeefabaddad2",
                "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
                "522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662",
                "76fc6ece0f4e1768cddf8853bb2d551b",
            ),
        ];
        for (key, iv, aad, plain, cipher, tag) in cases {
            let g = Gcm::new(&hex(key));
            let iv: [u8; 12] = hex(iv).try_into().unwrap();
            let mut buf = hex(plain);
            let t = g.seal(&iv, &hex(aad), &mut buf);
            assert_eq!(buf, hex(cipher));
            assert_eq!(t.to_vec(), hex(tag));
        }
    }
}

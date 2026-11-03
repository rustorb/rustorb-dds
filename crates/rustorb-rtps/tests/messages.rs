//! Tests of the message codec against byte layouts written out by hand
//! from the drawings in DDSI-RTPS 2.5 clause 9.4.

use rustorb_rtps::*;

const PREFIX: GuidPrefix = GuidPrefix([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
const WRITER: EntityId = EntityId::user([0, 0, 1], entity_kind::USER_WRITER_NO_KEY);
const READER: EntityId = EntityId::user([0, 0, 2], entity_kind::USER_READER_NO_KEY);

fn header_bytes() -> Vec<u8> {
    let mut b = b"RTPS".to_vec();
    b.extend_from_slice(&[2, 5, 0x01, 0x0f]);
    b.extend_from_slice(&PREFIX.0);
    b
}

fn header() -> Header {
    Header {
        version: ProtocolVersion::V2_5,
        vendor_id: VendorId([0x01, 0x0f]),
        guid_prefix: PREFIX,
    }
}

fn sn_set(base: i64, num_bits: u32, members: &[i64]) -> SequenceNumberSet {
    let mut s = SequenceNumberSet::new(SequenceNumber(base), num_bits).unwrap();
    for &m in members {
        assert!(s.insert(SequenceNumber(m)));
    }
    s
}

/// The payload of the example in 10.7 (ShapeType, CDR_LE).
const SHAPE_PAYLOAD: [u8; 28] = [
    0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x42, 0x4c, 0x55, 0x45, 0x00, 0x00, 0x00, 0x00,
    0x22, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00,
];

#[test]
fn decode_hand_written_big_endian() {
    let mut b = header_bytes();
    // INFO_DST, E=0
    b.extend_from_slice(&[0x0e, 0x00, 0x00, 12]);
    b.extend_from_slice(&[0xaa; 12]);
    // INFO_TS, E=0, I=0
    b.extend_from_slice(&[0x09, 0x00, 0x00, 8, 0, 0, 0, 10, 0x80, 0, 0, 0]);
    // DATA, E=0, D=1
    b.extend_from_slice(&[0x15, 0x04, 0x00, (20 + SHAPE_PAYLOAD.len()) as u8]);
    b.extend_from_slice(&[0, 0, 0, 16]); // extraFlags, octetsToInlineQos
    b.extend_from_slice(&READER.to_bytes());
    b.extend_from_slice(&WRITER.to_bytes());
    b.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 2]); // writerSN = 2^32 + 2
    b.extend_from_slice(&SHAPE_PAYLOAD);

    let msg = Message::decode(&b).unwrap();
    assert_eq!(msg.header, header());
    assert_eq!(
        msg.submessages,
        [
            Submessage::InfoDestination(InfoDestination {
                guid_prefix: GuidPrefix([0xaa; 12])
            }),
            Submessage::InfoTimestamp(InfoTimestamp {
                timestamp: Some(Time {
                    seconds: 10,
                    fraction: 0x8000_0000
                })
            }),
            Submessage::Data(Data {
                reader_id: READER,
                writer_id: WRITER,
                writer_sn: SequenceNumber((1 << 32) + 2),
                inline_qos: None,
                serialized_payload: Some(SerializedPayload {
                    kind: PayloadKind::Data,
                    bytes: SHAPE_PAYLOAD.to_vec(),
                }),
                non_standard_payload: false,
            }),
        ]
    );
    // Writing it again in big-endian gives back the same octets.
    assert_eq!(msg.encode(Endianness::Big).unwrap(), b);
}

#[test]
fn encode_heartbeat_little_endian_layout() {
    let mut msg = Message::new(header());
    msg.submessages.push(Submessage::Heartbeat(Heartbeat {
        final_flag: true,
        liveliness_flag: false,
        reader_id: EntityId::UNKNOWN,
        writer_id: WRITER,
        first_sn: SequenceNumber(1),
        last_sn: SequenceNumber(5),
        count: Count(3),
        group_info: None,
    }));
    let b = msg.encode(Endianness::Little).unwrap();
    let mut want = header_bytes();
    want.extend_from_slice(&[0x07, 0x03, 28, 0]); // E=1, F=1
    want.extend_from_slice(&[0, 0, 0, 0]);
    want.extend_from_slice(&WRITER.to_bytes());
    want.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0]);
    want.extend_from_slice(&[0, 0, 0, 0, 5, 0, 0, 0]);
    want.extend_from_slice(&[3, 0, 0, 0]);
    assert_eq!(b, want);
}

#[test]
fn sequence_number_set_bitmap_layout() {
    // The example of 9.4.2.6: "1234/12:00110".
    let mut set = sn_set(1234, 12, &[1236, 1237]);
    let mut msg = Message::new(header());
    msg.submessages.push(Submessage::AckNack(AckNack {
        final_flag: false,
        reader_id: READER,
        writer_id: WRITER,
        reader_sn_state: set,
        count: Count(1),
    }));
    let b = msg.encode(Endianness::Big).unwrap();
    let body = &b[Header::SIZE + 4..];
    assert_eq!(&body[8..16], &[0, 0, 0, 0, 0, 0, 0x04, 0xd2]); // bitmapBase
    assert_eq!(&body[16..20], &[0, 0, 0, 12]); // numBits
    assert_eq!(&body[20..24], &[0b0011_0000, 0, 0, 0]); // bitmap[0]
    assert_eq!(&body[24..28], &[0, 0, 0, 1]); // count
    assert_eq!(body.len(), 28);
    assert_eq!(
        set.iter().collect::<Vec<_>>(),
        [SequenceNumber(1236), SequenceNumber(1237)]
    );
    assert!(!set.contains(SequenceNumber(1234)));
    assert!(!set.insert(SequenceNumber(1246)));
}

fn every_kind() -> Vec<Submessage> {
    let mut qos = ParameterList::new();
    qos.push(Parameter::new(ParameterId(0x0070), [1, 2, 3, 4]));
    let mut frags = FragmentNumberSet::new(FragmentNumber(3), 40).unwrap();
    frags.insert(FragmentNumber(4));
    frags.insert(FragmentNumber(42));
    vec![
        Submessage::HeaderExtension(HeaderExtension {
            message_length: Some(1000),
            send_timestamp: Some(Time {
                seconds: 5,
                fraction: 6,
            }),
            u_extension4: Some([1, 2, 3, 4]),
            w_extension8: Some([8; 8]),
            checksum: Some(Checksum::Crc32([9, 9, 9, 9])),
            parameters: Some(qos.clone()),
        }),
        Submessage::Pad(Pad { len: 8 }),
        Submessage::Pad(Pad { len: 0 }),
        Submessage::InfoTimestamp(InfoTimestamp { timestamp: None }),
        Submessage::InfoTimestamp(InfoTimestamp {
            timestamp: Some(Time::INFINITE),
        }),
        Submessage::InfoSource(InfoSource {
            protocol_version: ProtocolVersion::V2_4,
            vendor_id: VendorId([1, 3]),
            guid_prefix: GuidPrefix([5; 12]),
        }),
        Submessage::InfoReplyIp4(InfoReplyIp4 {
            unicast_locator: LocatorUdpV4 {
                address: 0x0a00_0001,
                port: 7411,
            },
            multicast_locator: Some(LocatorUdpV4 {
                address: 0xefff_0001,
                port: 7400,
            }),
        }),
        Submessage::InfoDestination(InfoDestination {
            guid_prefix: GuidPrefix::UNKNOWN,
        }),
        Submessage::InfoReply(InfoReply {
            unicast_locators: vec![Locator::udpv4("10.0.0.1:7411".parse().unwrap())],
            multicast_locators: Some(vec![
                Locator::udpv4("239.255.0.1:7400".parse().unwrap()),
                Locator::udpv6("[ff02::1]:7400".parse().unwrap()),
            ]),
        }),
        Submessage::AckNack(AckNack {
            final_flag: true,
            reader_id: READER,
            writer_id: WRITER,
            reader_sn_state: sn_set(10, 256, &[10, 100, 265]),
            count: Count(-7),
        }),
        Submessage::Heartbeat(Heartbeat {
            final_flag: false,
            liveliness_flag: true,
            reader_id: READER,
            writer_id: WRITER,
            first_sn: SequenceNumber(11),
            last_sn: SequenceNumber(10),
            count: Count(1),
            group_info: Some(HeartbeatGroupInfo {
                current_gsn: SequenceNumber(30),
                first_gsn: SequenceNumber(20),
                last_gsn: SequenceNumber(25),
                writer_sets: Some((GroupDigest([1, 2, 3, 4]), GroupDigest([0; 4]))),
            }),
        }),
        Submessage::Gap(Gap {
            reader_id: EntityId::UNKNOWN,
            writer_id: WRITER,
            gap_start: SequenceNumber(3),
            gap_list: sn_set(7, 0, &[]),
            group_info: Some((SequenceNumber(4), SequenceNumber(9))),
            filtered_count: Some(ChangeCount(2)),
        }),
        Submessage::NackFrag(NackFrag {
            reader_id: READER,
            writer_id: WRITER,
            writer_sn: SequenceNumber(4),
            fragment_number_state: frags,
            count: Count(2),
        }),
        Submessage::HeartbeatFrag(HeartbeatFrag {
            reader_id: READER,
            writer_id: WRITER,
            writer_sn: SequenceNumber(4),
            last_fragment_num: FragmentNumber(17),
            count: Count(9),
        }),
        Submessage::DataFrag(DataFrag {
            key_flag: false,
            non_standard_payload: true,
            reader_id: READER,
            writer_id: WRITER,
            writer_sn: SequenceNumber(4),
            fragment_starting_num: FragmentNumber(2),
            fragments_in_submessage: 2,
            fragment_size: 8,
            sample_size: 30,
            inline_qos: Some(qos.clone()),
            serialized_payload: vec![0x5a; 16],
        }),
        Submessage::Data(Data {
            reader_id: READER,
            writer_id: WRITER,
            writer_sn: SequenceNumber(1),
            inline_qos: Some(qos),
            serialized_payload: Some(SerializedPayload {
                kind: PayloadKind::Key,
                bytes: vec![0, 1, 0, 0, 9, 9, 9, 9],
            }),
            non_standard_payload: false,
        }),
        Submessage::Data(Data {
            reader_id: EntityId::UNKNOWN,
            writer_id: EntityId::SPDP_BUILTIN_PARTICIPANT_ANNOUNCER,
            writer_sn: SequenceNumber(2),
            inline_qos: None,
            serialized_payload: None,
            non_standard_payload: false,
        }),
        Submessage::Unknown(UnknownSubmessage {
            id: 0x80,
            flags: 0xf1,
            body: vec![1, 2, 3, 4],
        }),
        Submessage::Data(Data {
            reader_id: READER,
            writer_id: WRITER,
            writer_sn: SequenceNumber(i64::MAX),
            inline_qos: None,
            serialized_payload: Some(SerializedPayload {
                kind: PayloadKind::Data,
                bytes: SHAPE_PAYLOAD.to_vec(),
            }),
            non_standard_payload: false,
        }),
    ]
}

#[test]
fn round_trip_every_kind_both_endians() {
    let mut msg = Message::new(header());
    msg.submessages = every_kind();
    for e in [Endianness::Big, Endianness::Little] {
        let b = msg.encode(e).unwrap();
        assert_eq!(b.len() % 4, 0);
        assert_eq!(Message::decode(&b).unwrap(), msg, "{e:?}");
    }
}

#[test]
fn mixed_endianness_in_one_message() {
    let mut big = Message::new(header());
    big.submessages = vec![Submessage::InfoDestination(InfoDestination {
        guid_prefix: GuidPrefix([3; 12]),
    })];
    let mut little = Message::new(header());
    little.submessages = vec![Submessage::AckNack(AckNack {
        final_flag: false,
        reader_id: READER,
        writer_id: WRITER,
        reader_sn_state: sn_set(5, 40, &[6, 44]),
        count: Count(1),
    })];
    let mut b = big.encode(Endianness::Big).unwrap();
    b.extend_from_slice(&little.encode(Endianness::Little).unwrap()[Header::SIZE..]);
    let msg = Message::decode(&b).unwrap();
    assert_eq!(msg.submessages[0], big.submessages[0]);
    assert_eq!(msg.submessages[1], little.submessages[0]);
}

#[test]
fn unknown_flags_and_ids_are_skipped() {
    let mut b = header_bytes();
    // A protocol id that 2.5 does not define, then a vendor-specific one.
    b.extend_from_slice(&[0x7f, 0x01, 4, 0, 1, 2, 3, 4]);
    b.extend_from_slice(&[0x81, 0x00, 0, 0]);
    b.extend_from_slice(&[0x01, 0xfe, 0, 0]); // PAD with unused flags set
    // INFO_DST with unused flags set.
    b.extend_from_slice(&[0x0e, 0xff, 12, 0]);
    b.extend_from_slice(&[0x11; 12]);
    let (_, subs) = Message::parse(&b).unwrap();
    let subs: Vec<_> = subs.collect::<Result<_, _>>().unwrap();
    assert_eq!(
        subs,
        [
            Submessage::Unknown(UnknownSubmessage {
                id: 0x7f,
                flags: 0x01,
                body: vec![1, 2, 3, 4],
            }),
            // octetsToNextHeader 0 on a non-PAD/INFO_TS submessage runs to
            // the end of the message.
            Submessage::Unknown(UnknownSubmessage {
                id: 0x81,
                flags: 0x00,
                body: b[Header::SIZE + 12..].to_vec(),
            }),
        ]
    );
}

#[test]
fn zero_length_pad_and_info_ts_are_empty() {
    let mut b = header_bytes();
    b.extend_from_slice(&[0x01, 0x01, 0, 0]); // PAD
    b.extend_from_slice(&[0x09, 0x03, 0, 0]); // INFO_TS, I=1
    b.extend_from_slice(&[0x0e, 0x01, 12, 0]);
    b.extend_from_slice(&[0x22; 12]);
    let msg = Message::decode(&b).unwrap();
    assert_eq!(
        msg.submessages,
        [
            Submessage::Pad(Pad { len: 0 }),
            Submessage::InfoTimestamp(InfoTimestamp { timestamp: None }),
            Submessage::InfoDestination(InfoDestination {
                guid_prefix: GuidPrefix([0x22; 12])
            }),
        ]
    );
}

#[test]
fn data_skips_unknown_elements_before_inline_qos() {
    let mut b = header_bytes();
    b.extend_from_slice(&[0x15, 0x07, 36, 0]); // E=1, Q=1, D=1
    b.extend_from_slice(&[0xff, 0xff, 20, 0]); // extraFlags set, octetsToInlineQos = 20
    b.extend_from_slice(&READER.to_bytes());
    b.extend_from_slice(&WRITER.to_bytes());
    b.extend_from_slice(&[0, 0, 0, 0, 7, 0, 0, 0]);
    b.extend_from_slice(&[0xee; 4]); // a future element
    b.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // inlineQos: sentinel only
    b.extend_from_slice(&[0, 1, 0, 0, 1, 2, 3, 4]);
    let msg = Message::decode(&b).unwrap();
    let Submessage::Data(d) = &msg.submessages[0] else {
        panic!()
    };
    assert_eq!(d.writer_sn, SequenceNumber(7));
    assert_eq!(d.inline_qos, Some(ParameterList::new()));
    assert_eq!(
        d.serialized_payload.as_ref().unwrap().bytes,
        [0, 1, 0, 0, 1, 2, 3, 4]
    );
}

#[test]
fn bad_headers() {
    assert_eq!(Message::decode(b"RTPS"), Err(DecodeError::UnexpectedEnd));
    let mut b = header_bytes();
    b[0] = b'X';
    assert!(matches!(Message::decode(&b), Err(DecodeError::NotRtps(_))));
    let mut b = header_bytes();
    b[4] = 3;
    assert!(matches!(
        Message::decode(&b),
        Err(DecodeError::UnsupportedVersion(_))
    ));
    // A newer minor version is accepted.
    let mut b = header_bytes();
    b[5] = 9;
    assert!(Message::decode(&b).is_ok());
}

/// After an invalid submessage, the ones before it are still delivered and
/// nothing after it is (8.3.4.1).
#[test]
fn invalid_submessage_ends_the_message() {
    let mut b = header_bytes();
    b.extend_from_slice(&[0x0e, 0x01, 12, 0]);
    b.extend_from_slice(&[0x22; 12]);
    // DATA with writerSN = 0.
    b.extend_from_slice(&[0x15, 0x05, 20, 0, 0, 0, 16, 0]);
    b.extend_from_slice(&[0; 16]);
    b.extend_from_slice(&[0x0e, 0x01, 12, 0]);
    b.extend_from_slice(&[0x33; 12]);
    let (_, subs) = Message::parse(&b).unwrap();
    let subs: Vec<_> = subs.collect();
    assert_eq!(subs.len(), 2);
    assert!(subs[0].is_ok());
    assert_eq!(
        subs[1],
        Err(DecodeError::InvalidSubmessage {
            id: submessage_id::DATA,
            reason: "writerSN is not positive"
        })
    );
    assert!(Message::decode(&b).is_err());
}

#[test]
fn invalid_cases() {
    fn first_error(sub: &[u8]) -> DecodeError {
        let mut b = header_bytes();
        b.extend_from_slice(sub);
        Message::decode(&b).unwrap_err()
    }
    // Header cut short.
    assert_eq!(
        first_error(&[0x0e, 0x01]),
        DecodeError::BadSubmessageLength { offset: 20 }
    );
    // Length past the end.
    assert_eq!(
        first_error(&[0x0e, 0x01, 16, 0, 0, 0, 0, 0]),
        DecodeError::BadSubmessageLength { offset: 20 }
    );
    // Length too small for the contents.
    assert!(matches!(
        first_error(&[0x0e, 0x01, 4, 0, 0, 0, 0, 0]),
        DecodeError::InvalidSubmessage { id: 0x0e, .. }
    ));
    // DATA with both D and K.
    let mut data = vec![0x15, 0x0d, 20, 0, 0, 0, 16, 0];
    data.extend_from_slice(&[0; 8]);
    data.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0]);
    assert!(matches!(
        first_error(&data),
        DecodeError::InvalidSubmessage { id: 0x15, .. }
    ));
    // DATA with octetsToInlineQos smaller than the fixed part.
    let mut data = vec![0x15, 0x01, 20, 0, 0, 0, 12, 0];
    data.extend_from_slice(&[0; 8]);
    data.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0]);
    assert!(matches!(
        first_error(&data),
        DecodeError::InvalidSubmessage { id: 0x15, .. }
    ));
    // DATA with Q set but no sentinel.
    let mut data = vec![0x15, 0x03, 24, 0, 0, 0, 16, 0];
    data.extend_from_slice(&[0; 8]);
    data.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0]);
    data.extend_from_slice(&[0x70, 0, 0, 0]);
    assert!(matches!(
        first_error(&data),
        DecodeError::InvalidSubmessage { id: 0x15, .. }
    ));
    // HEARTBEAT with lastSN < firstSN - 1.
    let mut hb = vec![0x07, 0x01, 28, 0];
    hb.extend_from_slice(&[0; 8]);
    hb.extend_from_slice(&[0, 0, 0, 0, 5, 0, 0, 0]);
    hb.extend_from_slice(&[0, 0, 0, 0, 3, 0, 0, 0]);
    hb.extend_from_slice(&[1, 0, 0, 0]);
    assert!(matches!(
        first_error(&hb),
        DecodeError::InvalidSubmessage { id: 0x07, .. }
    ));
    // ACKNACK with numBits > 256.
    let mut an = vec![0x06, 0x01, 24, 0];
    an.extend_from_slice(&[0; 8]);
    an.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0]);
    an.extend_from_slice(&[0x01, 0x01, 0, 0]);
    an.extend_from_slice(&[0; 4]);
    assert!(matches!(
        first_error(&an),
        DecodeError::InvalidSubmessage { id: 0x06, .. }
    ));
    // HeaderExtension that is not first.
    assert!(matches!(
        first_error(&[0x01, 0x01, 0, 0, 0x00, 0x01, 0, 0]),
        DecodeError::InvalidSubmessage { id: 0x00, .. }
    ));
    // HeaderExtension whose messageLength is too small.
    assert!(matches!(
        first_error(&[0x00, 0x03, 4, 0, 10, 0, 0, 0]),
        DecodeError::InvalidSubmessage { id: 0x00, .. }
    ));
}

#[test]
fn encode_errors() {
    let mut msg = Message::new(header());
    msg.submessages = vec![
        Submessage::Unknown(UnknownSubmessage {
            id: 0x90,
            flags: 0,
            body: vec![],
        }),
        Submessage::Pad(Pad { len: 4 }),
    ];
    assert_eq!(
        msg.encode(Endianness::Little),
        Err(EncodeError::EmptySubmessage { index: 0 })
    );

    msg.submessages = vec![
        Submessage::Pad(Pad { len: 0x1_0000 }),
        Submessage::Pad(Pad { len: 4 }),
    ];
    assert_eq!(
        msg.encode(Endianness::Little),
        Err(EncodeError::SubmessageTooLong { index: 0 })
    );

    msg.submessages = vec![
        Submessage::Pad(Pad { len: 4 }),
        Submessage::HeaderExtension(HeaderExtension::default()),
    ];
    assert_eq!(
        msg.encode(Endianness::Little),
        Err(EncodeError::MisplacedHeaderExtension { index: 1 })
    );
}

/// A last submessage longer than 65535 octets gets octetsToNextHeader = 0
/// (9.4.5.1.3).
#[test]
fn large_last_submessage() {
    let mut msg = Message::new(header());
    msg.submessages.push(Submessage::Data(Data {
        reader_id: READER,
        writer_id: WRITER,
        writer_sn: SequenceNumber(1),
        inline_qos: None,
        serialized_payload: Some(SerializedPayload {
            kind: PayloadKind::Data,
            bytes: vec![7; 70_000],
        }),
        non_standard_payload: false,
    }));
    let b = msg.encode(Endianness::Little).unwrap();
    assert_eq!(&b[Header::SIZE + 2..Header::SIZE + 4], &[0, 0]);
    assert_eq!(Message::decode(&b).unwrap(), msg);
}

#[test]
fn data_frag_drops_alignment_padding() {
    let mut msg = Message::new(header());
    let frag = DataFrag {
        key_flag: false,
        non_standard_payload: false,
        reader_id: READER,
        writer_id: WRITER,
        writer_sn: SequenceNumber(1),
        fragment_starting_num: FragmentNumber(3),
        fragments_in_submessage: 1,
        fragment_size: 10,
        sample_size: 25,
        inline_qos: None,
        serialized_payload: vec![1, 2, 3, 4, 5],
    };
    msg.submessages = vec![
        Submessage::DataFrag(frag.clone()),
        Submessage::Pad(Pad { len: 4 }),
    ];
    let b = msg.encode(Endianness::Big).unwrap();
    assert_eq!(Message::decode(&b).unwrap(), msg);
}

/// Feeds random and mutated input to the decoder. It must never panic, and
/// once decoded, encoding and decoding again must be stable.
#[test]
fn random_input_does_not_panic() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut msg = Message::new(header());
    msg.submessages = every_kind();
    let valid = [
        msg.encode(Endianness::Big).unwrap(),
        msg.encode(Endianness::Little).unwrap(),
    ];
    for i in 0..20_000 {
        let mut b = if i % 4 == 0 {
            let mut b = header_bytes();
            let n = (rand() % 200) as usize;
            b.extend((0..n).map(|_| rand() as u8));
            b
        } else {
            valid[i % 2].clone()
        };
        if b.len() == Header::SIZE {
            continue;
        }
        let flips = 1 + rand() % 4;
        for _ in 0..flips {
            let at = Header::SIZE + (rand() as usize) % (b.len() - Header::SIZE);
            b[at] = rand() as u8;
        }
        if rand() % 3 == 0 {
            let cut = Header::SIZE + (rand() as usize) % (b.len() - Header::SIZE);
            b.truncate(cut);
        }
        if let Ok(m1) = Message::decode(&b) {
            check_stable(&m1);
        }
        let (_, subs) = Message::parse(&b).unwrap();
        for s in subs {
            let _ = s;
        }
    }
}

fn check_stable(m1: &Message) {
    for e in [Endianness::Big, Endianness::Little] {
        let Ok(b2) = m1.encode(e) else {
            continue;
        };
        // A HeaderExtension's messageLength is kept as read, so the
        // re-encoded message may no longer satisfy it.
        let Ok(m2) = Message::decode(&b2) else {
            continue;
        };
        let b3 = m2.encode(e).unwrap();
        assert_eq!(Message::decode(&b3).unwrap(), m2);
    }
}

/// An ACKNACK whose readerSNState has base 0 and no bits, as some
/// implementations send before they have received anything: taken as
/// base 1. Base 0 with bits stays invalid (8.3.5.5).
#[test]
fn acknack_with_empty_base_zero() {
    let ack = |num_bits: u32| {
        let mut b = header_bytes();
        // ACKNACK, little endian, not final, 24 octets.
        b.extend_from_slice(&[0x06, 0x01, 24 + 4 * num_bits.div_ceil(32) as u8, 0]);
        b.extend_from_slice(&READER.to_bytes());
        b.extend_from_slice(&WRITER.to_bytes());
        b.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]); // base 0
        b.extend_from_slice(&num_bits.to_le_bytes());
        if num_bits > 0 {
            b.extend_from_slice(&[0, 0, 0, 0x80]);
        }
        b.extend_from_slice(&3i32.to_le_bytes());
        b
    };
    let msg = Message::decode(&ack(0)).unwrap();
    let Submessage::AckNack(a) = &msg.submessages[0] else {
        panic!("{:?}", msg.submessages)
    };
    assert_eq!(a.reader_sn_state.base(), SequenceNumber(1));
    assert_eq!(a.reader_sn_state.num_bits(), 0);
    assert_eq!(a.count, Count(3));
    assert!(!a.final_flag);
    assert!(matches!(
        Message::decode(&ack(1)),
        Err(DecodeError::InvalidSubmessage { id: 0x06, .. })
    ));
}

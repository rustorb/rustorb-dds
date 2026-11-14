//! A recursive type (DDS-XTypes 1.3, 7.3.4.8): `Tree::Node` of
//! interop/idl/Tree.idl, a node with a sequence of nodes, written and
//! read as DynamicData (rustorb-dds#70).
//!
//! ```text
//! tree pub <domain> <count> [discover]
//! tree sub <domain> <count> <timeout_s> [discover]
//! ```
//!
//! The topic is `tree`, RELIABLE and KEEP_ALL, in XCDR2. With `discover`
//! the program does not build the type: it takes it from the other
//! side's endpoint with TypeLookup, which serves a recursive type by its
//! strongly connected component. Sample `n` is the tree
//! `n (10n + 1, 10n + 2 (100n))`. The publisher waits for a reader,
//! writes `count` samples and waits for acknowledgments; the subscriber
//! exits once it has `count` samples, the expected ones in order, or with
//! an error after `timeout_s` seconds or on a wrong sample.

use std::time::{Duration, Instant};

use rustorb_dds::{
    DataReaderQos, DataWriterQos, DomainParticipant, DynamicData, DynamicDataFactory, DynamicType,
    History, Reliability, data_representation,
};
use rustorb_xtypes::types::{AliasType, Extensibility, Primitive, StructMember, StructType, Type};

/// `Tree::Node`, as interop/idl/Tree.idl declares it.
fn node_type() -> DynamicType {
    let fwd = Type::forward("Tree::Node");
    let seq = Type::from(AliasType {
        name: "Tree::Forest".into(),
        related: Type::sequence(fwd.clone()),
    });
    let node = Type::from(StructType {
        name: "Tree::Node".into(),
        extensibility: Extensibility::Appendable,
        members: vec![
            StructMember::new("data", 0, Type::Primitive(Primitive::Int32)),
            StructMember::new("children", 1, seq),
        ],
        ..StructType::default()
    });
    fwd.define(node.clone())
        .expect("Tree::Node is defined once");
    DynamicType::from(node)
}

/// Sets `d`, a node, to `data` with children of the given data, each a
/// leaf but the last, which has `grandchild` as its only child.
fn set_node(
    d: &mut DynamicData,
    data: i32,
    children: &[i32],
    grandchild: Option<i32>,
) -> rustorb_dds::Result<()> {
    d.set_int32_value(0, data)?;
    let seq = d.loan_value(1)?;
    for (i, c) in children.iter().enumerate() {
        let child = seq.loan_value(i as u32)?;
        child.set_int32_value(0, *c)?;
        if let (true, Some(g)) = (i + 1 == children.len(), grandchild) {
            child.loan_value(1)?.loan_value(0)?.set_int32_value(0, g)?;
        }
    }
    Ok(())
}

/// The tree of a node, as `data (child, child...)`.
fn text(d: &DynamicData) -> rustorb_dds::Result<String> {
    let mut out = d.get_int32_value(0)?.to_string();
    let seq = d.get_value(1)?;
    let n = seq.get_item_count();
    if n > 0 {
        let children = (0..n)
            .map(|i| text(seq.get_value(i)?))
            .collect::<rustorb_dds::Result<Vec<_>>>()?;
        out += &format!(" ({})", children.join(", "));
    }
    Ok(out)
}

/// Sample `n`: `n (10n + 1, 10n + 2 (100n))`.
fn sample(ty: &DynamicType, n: i32) -> rustorb_dds::Result<DynamicData> {
    let mut d = DynamicDataFactory::create_data(ty);
    set_node(&mut d, n, &[10 * n + 1, 10 * n + 2], Some(100 * n))?;
    Ok(d)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: tree pub <domain> <count> [discover]\n       \
                 tree sub <domain> <count> <timeout_s> [discover]";
    if args.len() < 4 || (args[1] != "pub" && args[1] != "sub") {
        return Err(usage.into());
    }
    let sub = args[1] == "sub";
    let domain: u32 = args[2].parse()?;
    let count: i32 = args[3].parse()?;
    let wait = if sub {
        Duration::from_secs(args.get(4).ok_or(usage)?.parse()?)
    } else {
        Duration::from_secs(60)
    };
    let discover = args.get(if sub { 5 } else { 4 }).map(String::as_str) == Some("discover");
    let deadline = Instant::now() + wait;
    let participant = DomainParticipant::new(domain)?;
    let ty = if discover {
        println!("rustorb-dds: waiting for the type of tree...");
        let ty = participant
            .discover_topic_type("tree", wait)?
            .ok_or("rustorb-dds: the type of the topic did not come")?;
        println!("rustorb-dds: got type {}", ty.get_name());
        ty
    } else {
        node_type()
    };
    let topic = participant.create_dynamic_topic("tree", &ty)?;
    if !sub {
        let qos = DataWriterQos {
            reliability: Reliability::reliable(),
            history: History::KeepAll,
            data_representation: vec![data_representation::XCDR2],
            ..DataWriterQos::default()
        };
        let writer = participant
            .create_publisher()?
            .create_datawriter_with_qos(&topic, qos)?;
        println!("rustorb-dds: waiting for a subscriber...");
        while !writer.wait_for_subscriptions(1, Duration::from_secs(1)) {}
        std::thread::sleep(Duration::from_millis(500));
        for n in 1..=count {
            writer.write(&sample(&ty, n)?)?;
            println!("rustorb-dds: sent {n}");
            std::thread::sleep(Duration::from_millis(20));
        }
        writer.wait_for_acknowledgments(Duration::from_secs(30))?;
        println!("rustorb-dds: all acknowledged");
    } else {
        let qos = DataReaderQos {
            reliability: Reliability::reliable(),
            history: History::KeepAll,
            data_representation: vec![data_representation::XCDR2],
            ..DataReaderQos::default()
        };
        let reader = participant
            .create_subscriber()?
            .create_datareader_with_qos(&topic, qos)?;
        let mut got = 0;
        while got < count {
            if Instant::now() > deadline {
                return Err(format!("rustorb-dds: got {got} of {count} samples").into());
            }
            let samples: Vec<DynamicData> =
                reader.take().into_iter().filter_map(|s| s.data).collect();
            if samples.is_empty() {
                std::thread::sleep(Duration::from_millis(20));
            }
            for s in samples {
                got += 1;
                let (have, want) = (text(&s)?, text(&sample(&ty, got)?)?);
                if have != want {
                    return Err(format!("rustorb-dds: got the tree {have}, want {want}").into());
                }
                println!("rustorb-dds: received {got}: {have}");
            }
        }
    }
    println!(
        "rustorb-dds: {} TypeObjects came by TypeLookup",
        participant.learnt_types()
    );
    Ok(())
}

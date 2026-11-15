//! Requests and replies (DDS-RPC 1.0): requesters and repliers over the
//! related sample identity, between participants and within one.

#[path = "../examples/keyed/shape.rs"]
mod shape;

/// `rustorb-idl -brust -Wbdds_only -Wbxtypes
/// -Wbextern=dds=::rustorb_dds::rpc::idl::dds rpc_basic/Sum.idl`, which
/// implements `BasicRequest` and `BasicReply` for the `@RPCRequestType`
/// and `@RPCReplyType` structs.
#[allow(dead_code, clippy::all)]
#[rustfmt::skip]
#[path = "rpc_basic/sum.rs"]
mod sum;

use std::time::Duration;

use rustorb_dds::rpc::{RemoteExceptionCode, Replier, ReplyHeader, Requester, RpcConfig};
use rustorb_dds::{DomainParticipant, Error, ParticipantConfig};
use shape::Shape;
use sum::calc::{SumReply, SumRequest};

const WAIT: Duration = Duration::from_secs(10);

fn participant(domain: u32) -> DomainParticipant {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    DomainParticipant::with_config(c).unwrap()
}

fn shape(id: u32, index: u32) -> Shape {
    Shape {
        id,
        index,
        message: format!("request {index}"),
    }
}

/// Answers every request with the same shape, `index` doubled, until
/// `n` requests are answered.
fn serve(replier: &Replier<Shape, Shape>, n: usize) {
    let mut done = 0;
    while done < n {
        for r in replier.receive_requests(WAIT).unwrap() {
            let reply = Shape {
                index: r.data.index * 2,
                message: format!("reply {}", r.data.index),
                ..r.data
            };
            replier.send_reply(&reply, &r.id).unwrap();
            done += 1;
        }
    }
}

#[test]
fn replies_go_to_their_requests() {
    let (a, b) = (participant(81), participant(81));
    let replier = Replier::<Shape, Shape>::new(&b, "Double").unwrap();
    let requester = Requester::<Shape, Shape>::new(&a, "Double").unwrap();
    assert!(requester.wait_for_service(WAIT));
    let server = std::thread::spawn(move || serve(&replier, 3));
    let ids: Vec<_> = (1..=3)
        .map(|i| requester.send_request(&shape(7, i)).unwrap())
        .collect();
    assert_eq!(ids[0].writer_guid, requester.request_datawriter().guid());
    // Out of order: each reply is found by its request.
    for (i, id) in ids.iter().enumerate().rev() {
        let reply = requester.receive_reply(id, WAIT).unwrap();
        assert_eq!(reply.index, 2 * (i as u32 + 1));
        assert_eq!(reply.message, format!("reply {}", i + 1));
    }
    server.join().unwrap();
    assert!(matches!(
        requester.receive_reply(&ids[0], Duration::from_millis(100)),
        Err(Error::Timeout)
    ));
}

#[test]
fn requesters_get_only_their_replies() {
    let p = participant(82);
    let q = participant(82);
    let config = RpcConfig::with_topics("rq/addRequest", "rr/addReply");
    let replier = Replier::<Shape, Shape>::with_config(&p, &config).unwrap();
    let first = Requester::<Shape, Shape>::with_config(&q, &config).unwrap();
    let second = Requester::<Shape, Shape>::with_config(&p, &config).unwrap();
    assert!(first.wait_for_service(WAIT));
    assert!(second.wait_for_service(WAIT));
    let server = std::thread::spawn(move || serve(&replier, 4));
    let a1 = first.send_request(&shape(1, 1)).unwrap();
    let b1 = second.send_request(&shape(2, 10)).unwrap();
    let a2 = first.send_request(&shape(1, 2)).unwrap();
    let b2 = second.send_request(&shape(2, 20)).unwrap();
    assert_eq!(second.receive_reply(&b2, WAIT).unwrap().index, 40);
    assert_eq!(first.receive_reply(&a2, WAIT).unwrap().index, 4);
    server.join().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let all = |r: &Requester<Shape, Shape>| {
        r.take_all_replies()
            .unwrap()
            .into_iter()
            .map(|(id, s)| (id, s.index))
            .collect::<Vec<_>>()
    };
    assert_eq!(all(&first), vec![(a1, 2)]);
    assert_eq!(all(&second), vec![(b1, 20)]);
}

#[test]
fn request_waits_for_the_reply() {
    let p = participant(83);
    let replier = Replier::<Shape, Shape>::new(&p, "Echo").unwrap();
    let requester = Requester::<Shape, Shape>::new(&p, "Echo").unwrap();
    assert!(requester.wait_for_service(WAIT));
    let server = std::thread::spawn(move || serve(&replier, 1));
    assert_eq!(requester.request(&shape(3, 4), WAIT).unwrap().index, 8);
    server.join().unwrap();
}

#[test]
fn no_service_no_match() {
    let p = participant(84);
    let requester = Requester::<Shape, Shape>::new(&p, "Nobody").unwrap();
    assert!(!requester.wait_for_service(Duration::from_millis(500)));
}

#[test]
fn replies_are_sent_to_the_requester_only() {
    let p = participant(26);
    let q = participant(26);
    let replier = Replier::<Shape, Shape>::new(&p, "Directed").unwrap();
    let asking = Requester::<Shape, Shape>::new(&q, "Directed").unwrap();
    let local = Requester::<Shape, Shape>::new(&p, "Directed").unwrap();
    let remote = Requester::<Shape, Shape>::new(&q, "Directed").unwrap();
    for r in [&asking, &local, &remote] {
        assert!(r.wait_for_service(WAIT));
    }
    let server = std::thread::spawn(move || serve(&replier, 1));
    assert_eq!(asking.request(&shape(5, 3), WAIT).unwrap().index, 6);
    server.join().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    // The others' readers did not get the reply at all, neither in the
    // participant of the replier nor in that of the requester.
    // (The replier is gone: what they have tells only that.)
    let values = |r: &Requester<Shape, Shape>| {
        r.reply_datareader()
            .take()
            .into_iter()
            .filter_map(|s| s.data)
            .count()
    };
    assert_eq!(values(&local), 0);
    assert_eq!(values(&remote), 0);
}

#[test]
fn a_service_answers_on_the_aliases_of_its_topics() {
    let (a, b) = (participant(27), participant(27));
    // A service of an interface Calculator derived from Adder: its
    // topics are known by the names of Adder's too (DDS-RPC 1.0, 7.4.3).
    let mut config = RpcConfig::new("Calculator_Service");
    config.request_topic_aliases = vec!["Adder_Service_Request".into()];
    config.reply_topic_aliases = vec!["Adder_Service_Reply".into()];
    let replier = Replier::<Shape, Shape>::with_config(&b, &config).unwrap();
    let remote = Requester::<Shape, Shape>::new(&a, "Adder_Service").unwrap();
    let local = Requester::<Shape, Shape>::new(&b, "Adder_Service").unwrap();
    assert!(remote.wait_for_service(WAIT));
    assert!(local.wait_for_service(WAIT));
    let server = std::thread::spawn(move || serve(&replier, 2));
    assert_eq!(remote.request(&shape(1, 2), WAIT).unwrap().index, 4);
    assert_eq!(local.request(&shape(1, 5), WAIT).unwrap().index, 10);
    server.join().unwrap();
    // Others do not match.
    let other = Requester::<Shape, Shape>::new(&a, "Subtractor_Service").unwrap();
    assert!(!other.wait_for_service(Duration::from_millis(500)));
}

#[test]
fn basic_mapping_headers() {
    let (a, b) = (participant(28), participant(28));
    let mut config = RpcConfig::new("Sum");
    config.service_instance_name = Some("first".into());
    let replier = Replier::<SumRequest, SumReply>::basic(&b, &config).unwrap();
    let mut other_config = config.clone();
    other_config.service_instance_name = Some("second".into());
    let other = Replier::<SumRequest, SumReply>::basic(&b, &other_config).unwrap();
    let requester = Requester::<SumRequest, SumReply>::basic(&a, &config).unwrap();
    let second = Requester::<SumRequest, SumReply>::basic(&a, &RpcConfig::new("Sum")).unwrap();
    assert!(requester.wait_for_service(WAIT));
    assert!(second.wait_for_service(WAIT));
    let request = |a, b| SumRequest {
        a,
        b,
        ..SumRequest::default()
    };
    let id = requester.send_request(&request(2, 3)).unwrap();
    assert_eq!(id.writer_guid, requester.request_datawriter().guid());
    // The replier sees the identity of the header; the one of the other
    // instance does not see the request.
    let got = replier.receive_requests(WAIT).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].id, id);
    assert_eq!(got[0].data.header.instance_name, "first");
    let reply = SumReply {
        header: ReplyHeader {
            remote_ex: RemoteExceptionCode::RemoteExUnsupported,
            ..ReplyHeader::default()
        },
        sum: got[0].data.a + got[0].data.b,
    };
    replier.send_reply(&reply, &got[0].id).unwrap();
    let r = requester.receive_reply(&id, WAIT).unwrap();
    assert_eq!(r.sum, 5);
    assert_eq!(rustorb_rtps_id(&r.header), id);
    assert_eq!(r.header.remote_ex, RemoteExceptionCode::RemoteExUnsupported);
    assert!(other.take_requests().unwrap().is_empty());
    // A request with no instance name goes to both; each requester gets
    // the replies to its own only.
    let id2 = second.send_request(&request(1, 1)).unwrap();
    for rep in [&replier, &other] {
        let got = rep.receive_requests(WAIT).unwrap();
        assert_eq!(got.len(), 1);
        let reply = SumReply {
            sum: got[0].data.a + got[0].data.b,
            ..SumReply::default()
        };
        rep.send_reply(&reply, &got[0].id).unwrap();
    }
    let replies = |r: &Requester<SumRequest, SumReply>, n: usize| {
        let mut all = Vec::new();
        let deadline = std::time::Instant::now() + WAIT;
        while all.len() < n && std::time::Instant::now() < deadline {
            all.extend(r.take_all_replies().unwrap());
            std::thread::sleep(Duration::from_millis(20));
        }
        all.into_iter()
            .map(|(id, r)| (id, r.sum))
            .collect::<Vec<_>>()
    };
    assert_eq!(replies(&second, 2), vec![(id2, 2), (id2, 2)]);
    std::thread::sleep(Duration::from_millis(300));
    assert!(requester.take_all_replies().unwrap().is_empty());
}

/// The request a reply header names.
fn rustorb_rtps_id(h: &ReplyHeader) -> rustorb_dds::SampleIdentity {
    h.related_request_id.into()
}

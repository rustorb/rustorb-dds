//! Requests and replies (DDS-RPC 1.0): requesters and repliers over the
//! related sample identity, between participants and within one.

#[path = "../examples/keyed/shape.rs"]
mod shape;

use std::time::Duration;

use rustorb_dds::rpc::{Replier, Requester, RpcConfig};
use rustorb_dds::{DomainParticipant, Error, ParticipantConfig};
use shape::Shape;

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

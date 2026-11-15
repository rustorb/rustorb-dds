//! The function-call style of DDS-RPC 1.0: the clients and servers that
//! `rustorb-idl -brust -Wbdds_only -Wbxtypes` writes for `@DDSService`
//! interfaces (`rpc_service/Calculator.idl`).

#[allow(dead_code, clippy::all)]
#[rustfmt::skip]
#[path = "rpc_service/calculator.rs"]
mod calculator;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use calculator::calc::{
    Adder, AdderAddException, AdderClient, Calculator, CalculatorClient, CalculatorDivideOut,
    CalculatorServer, TooBigEx,
};
use rustorb_dds::rpc::{RemoteExceptionCode, RpcError};
use rustorb_dds::{DomainParticipant, ParticipantConfig};

const WAIT: Duration = Duration::from_secs(10);

fn participant(domain: u32) -> DomainParticipant {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    DomainParticipant::with_config(c).unwrap()
}

#[derive(Debug, Default)]
struct Service {
    name: Mutex<String>,
    calls: AtomicU32,
    notes: Mutex<Vec<String>>,
}

impl Service {
    fn called(&self) {
        self.calls.fetch_add(1, Ordering::Relaxed);
    }
}

impl Adder for Service {
    fn add(&self, a: i32, b: i32) -> Result<i32, RpcError<AdderAddException>> {
        self.called();
        let sum = a + b;
        if sum > 100 {
            return Err(RpcError::Exception(AdderAddException::TooBig(TooBigEx {
                limit: 100,
            })));
        }
        Ok(sum)
    }
}

impl Calculator for Service {
    fn divide(
        &self,
        a: f64,
        b: f64,
    ) -> Result<CalculatorDivideOut, RpcError<std::convert::Infallible>> {
        self.called();
        if b == 0.0 {
            return Err(RpcError::Remote(
                RemoteExceptionCode::RemoteExInvalidArgument,
            ));
        }
        Ok(CalculatorDivideOut {
            remainder: a % b,
            r#return: (a / b).trunc(),
        })
    }

    fn clear(
        &self,
        log: String,
    ) -> Result<calculator::calc::CalculatorClearOut, RpcError<std::convert::Infallible>> {
        self.called();
        Ok(calculator::calc::CalculatorClearOut {
            log: format!("{log} cleared"),
        })
    }

    fn note(&self, text: String) -> Result<(), RpcError<std::convert::Infallible>> {
        self.notes.lock().unwrap().push(text);
        Ok(())
    }

    fn get_attribute_name(&self) -> Result<String, RpcError<std::convert::Infallible>> {
        Ok(self.name.lock().unwrap().clone())
    }

    fn set_attribute_name(&self, name: String) -> Result<(), RpcError<std::convert::Infallible>> {
        *self.name.lock().unwrap() = name;
        Ok(())
    }

    fn get_attribute_calls(&self) -> Result<u32, RpcError<std::convert::Infallible>> {
        Ok(self.calls.load(Ordering::Relaxed))
    }
}

#[test]
fn clients_call_the_operations_of_a_service() {
    let (a, b) = (participant(29), participant(29));
    let server = CalculatorServer::new(&b, Service::default()).unwrap();
    let stop = AtomicBool::new(false);
    std::thread::scope(|s| {
        s.spawn(|| server.run(&stop).unwrap());
        let client = CalculatorClient::new(&a).unwrap();
        assert!(client.wait_for_service(WAIT));
        assert_eq!(client.add(2, 3).unwrap(), 5);
        assert!(matches!(
            client.add(60, 70),
            Err(RpcError::Exception(AdderAddException::TooBig(TooBigEx {
                limit: 100
            })))
        ));
        let out = client.divide(7.0, 2.0).unwrap();
        assert_eq!((out.r#return, out.remainder), (3.0, 1.0));
        assert!(matches!(
            client.divide(1.0, 0.0),
            Err(RpcError::Remote(
                RemoteExceptionCode::RemoteExInvalidArgument
            ))
        ));
        assert_eq!(client.clear("log".into()).unwrap().log, "log cleared");
        client.set_attribute_name("calc".into()).unwrap();
        assert_eq!(client.get_attribute_name().unwrap(), "calc");
        client.note("one-way".into()).unwrap();
        assert_eq!(client.get_attribute_calls().unwrap(), 5);
        // A client of the base interface calls the derived service, on
        // the aliases of its topics (DDS-RPC 1.0, 7.4.3).
        let adder = AdderClient::new(&a).unwrap();
        assert!(adder.wait_for_service(WAIT));
        assert_eq!(adder.add(40, 2).unwrap(), 42);
        stop.store(true, Ordering::Relaxed);
    });
    assert_eq!(
        *server.service().notes.lock().unwrap(),
        vec!["one-way".to_string()]
    );
}

#[test]
fn services_are_found_by_their_names() {
    let p = participant(30);
    let server = CalculatorServer::with_service_name(&p, "second", Service::default()).unwrap();
    let other = CalculatorClient::new(&p).unwrap();
    let client = CalculatorClient::with_service_name(&p, "second").unwrap();
    assert!(client.wait_for_service(WAIT));
    assert!(!other.wait_for_service(Duration::from_millis(500)));
    assert_eq!(
        CalculatorClient::config("second").request_topic,
        "Calc_Calculator_second_Request"
    );
    std::thread::scope(|s| {
        s.spawn(|| while server.process(Duration::from_millis(50)).unwrap() == 0 {});
        assert_eq!(client.add(1, 1).unwrap(), 2);
    });
}

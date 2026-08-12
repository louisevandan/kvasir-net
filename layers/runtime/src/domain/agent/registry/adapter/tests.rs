use super::{Adapter, RegistrationRefusal, authorize_registration};
use crate::foundation::transport::tcp;

fn adapter(endpoint: &str) -> Adapter {
    Adapter {
        kind: "test".into(),
        transport: tcp(endpoint),
        endpoint: Some(endpoint.into()),
        descriptor: "{}".into(),
    }
}

#[test]
fn a_first_registration_is_always_accepted() {
    assert!(authorize_registration(None, "127.0.0.1:19203", 0).is_ok());
    assert!(authorize_registration(None, "127.0.0.1:19203", 7).is_ok());
}

#[test]
fn re_registering_the_same_endpoint_is_idempotent() {
    let existing = adapter("127.0.0.1:19203");
    assert!(authorize_registration(Some(&existing), "127.0.0.1:19203", 4).is_ok());
}

#[test]
fn moving_the_endpoint_is_allowed_only_while_no_slot_is_attached() {
    let existing = adapter("127.0.0.1:19203");
    assert!(authorize_registration(Some(&existing), "127.0.0.1:19999", 0).is_ok());
}

#[test]
fn moving_the_endpoint_under_attached_slots_is_refused() {
    let existing = adapter("127.0.0.1:19203");
    let refusal = authorize_registration(Some(&existing), "127.0.0.1:19999", 2).unwrap_err();
    assert_eq!(
        refusal,
        RegistrationRefusal::EndpointReboundWhileAttached {
            registered: "127.0.0.1:19203".into(),
            attached_nodes: 2,
        }
    );
    assert!(refusal.detail().contains("127.0.0.1:19203"));
}

#[test]
fn a_co_resident_adapter_cannot_be_replaced_by_a_wire_endpoint_while_attached() {
    let existing = Adapter {
        kind: "test".into(),
        transport: tcp("127.0.0.1:1"),
        endpoint: None,
        descriptor: "{}".into(),
    };
    assert!(matches!(
        authorize_registration(Some(&existing), "127.0.0.1:19203", 1),
        Err(RegistrationRefusal::EndpointReboundWhileAttached { .. })
    ));
}

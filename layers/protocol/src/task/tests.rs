use super::*;

fn participant(role: ParticipantRole, id: &str) -> Participant {
    Participant {
        agent_id: "agent-a".into(),
        role,
        instance_id: id.into(),
    }
}

#[test]
fn derives_local_external_controller_route() {
    let task = TaskEnvelope::new(
        "task-1",
        None,
        participant(ParticipantRole::External, "client-1"),
        participant(ParticipantRole::Controller, "controller-1"),
        Message::InventoryQuery {
            controller_id: "controller-1".into(),
            request_id: "request-1".into(),
        },
    )
    .unwrap();
    assert_eq!(task.direction, TaskDirection::ExternalController);
    assert!(task.is_local_bypass());
}

#[test]
fn rejects_a_message_on_the_wrong_direction() {
    let result = TaskEnvelope::new(
        "task-1",
        None,
        participant(ParticipantRole::Controller, "controller-1"),
        participant(ParticipantRole::Node, "node-1"),
        Message::HardwareReport {
            agent_id: "agent-a".into(),
            report_id: "request-1".into(),
            snapshot: "{}".into(),
        },
    );
    assert!(result.is_err());
}

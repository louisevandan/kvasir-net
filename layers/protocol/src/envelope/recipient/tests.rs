use super::*;

#[test]
fn the_agent_keeps_what_belongs_to_the_machine_and_the_registry() {
    assert!(Recipient::Agent.is_agent());
    assert_eq!(Recipient::Agent.node_id(), None);
}

#[test]
fn a_node_recipient_names_which_node() {
    let recipient = Recipient::node("node-a");
    assert!(!recipient.is_agent());
    assert_eq!(recipient.node_id(), Some("node-a"));
}

#[test]
fn two_nodes_on_one_agent_are_distinct_recipients() {
    assert_ne!(Recipient::node("node-a"), Recipient::node("node-b"));
}

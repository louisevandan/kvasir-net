//! Standing a deployment up: a node per stage, then a load on each.
//!
//! Separate from running work on it, because the two change for different
//! reasons — this follows the lifecycle vocabulary and what a load is allowed
//! to report, while its parent follows what an inference does. It is also the
//! half that must fail loudly: a run against a deployment that never bound
//! would otherwise look like a deployment that produced nothing.
//!
//! Every step here walks the whole fleet. A replica is a deployment like any
//! other: its nodes are created and loaded on their own, and a replica that
//! failed to bind is named by which one it was, because "a stage never bound"
//! on a fleet holding the same stage four times is not something an operator
//! can act on.

use super::Session;
use crate::fleet::Fleet;
use p4_protocol::{Chain, Link, QueueClass, Recipient};
use p4_service::cache::{CacheTransaction, CacheTransactionKind, CacheTransactionState};
use p4_service::message::wire::{encode_to_agent, encode_to_node};
use p4_service::message::{ToAgent, ToNode};
use std::collections::HashMap;
use std::sync::atomic::Ordering::SeqCst;

/// Every (deployment, stage) pair in the fleet, in the order they are stood up.
pub(super) fn places(fleet: &Fleet) -> impl Iterator<Item = (usize, usize)> + '_ {
    (0..fleet.deployments().len())
        .flat_map(|deployment| (0..fleet.stages()).map(move |stage| (deployment, stage)))
}

impl Session {
    pub async fn create_nodes(
        &self,
        fleet: &Fleet,
        adapter: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let nodes = fleet.deployments().len() * fleet.stages();
        for (deployment, stage) in places(fleet) {
            self.send(
                fleet.deployments()[deployment][stage].clone(),
                Recipient::Agent,
                QueueClass::Control,
                &self.route(&format!("create-{deployment}-{stage}")),
                None,
                encode_to_agent(&ToAgent::CreateNode {
                    node: fleet.node_of(deployment, stage),
                    adapter: adapter.to_owned(),
                }),
            )?;
        }
        if !self
            .until(|| self.replies.accepted.load(SeqCst) >= nodes)
            .await
        {
            return Err(self.gave_up("creating nodes").into());
        }
        for (deployment, stage) in places(fleet) {
            let stream = self.stream(&self.route(&format!("create-{deployment}-{stage}")));
            if !stream.accepted {
                return Err(format!(
                    "deployment {deployment} stage {stage} refused the node: {}",
                    stream.failed.unwrap_or_else(|| "no answer".into())
                )
                .into());
            }
        }
        Ok(())
    }

    pub async fn load(
        &self,
        fleet: &Fleet,
        ceiling: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let nodes = fleet.deployments().len() * fleet.stages();
        for (deployment, stage) in places(fleet) {
            let address = fleet.deployments()[deployment][stage].clone();
            let node = fleet.node_of(deployment, stage);
            let (capability_snapshot_id, capability_expires_at) = self.capability_for(&address);
            if !Session::capability_is_valid(
                &capability_snapshot_id,
                capability_expires_at,
                Session::unix_ms(),
            ) {
                return Err(format!(
                    "distributed load requires a live capability snapshot for {address}"
                )
                .into());
            }
            let single = Chain::new(vec![Link {
                address: address.clone(),
                node: node.clone(),
                binding: "deployment".into(),
                generation: 1,
            }])?;
            self.send(
                address.clone(),
                Recipient::node(node),
                QueueClass::Control,
                &self.route(&format!("load-{deployment}-{stage}")),
                Some(single),
                encode_to_node(&ToNode::Load {
                    // A mock ignores this; a concrete backend reads it and is
                    // the only thing that knows what it means. The driver
                    // carries it rather than inventing one, because a plan is
                    // opaque above the adapter and inventing one here would be
                    // this tool knowing a backend.
                    plan: self.plans[deployment][stage].clone(),
                    artifact: self.artifact(),
                    ceiling,
                    capability_snapshot_id,
                    capability_expires_at,
                }),
            )?;
            self.trace(format!(
                "load_sent deployment={deployment} stage={stage} address={address}"
            ));
        }
        let load_routes: Vec<_> = places(fleet)
            .map(|(deployment, stage)| self.route(&format!("load-{deployment}-{stage}")))
            .collect();
        if !self
            .until(|| {
                self.replies.bound.load(SeqCst) >= nodes
                    || load_routes
                        .iter()
                        .any(|route| self.stream(route).failed.is_some())
            })
            .await
        {
            self.trace(format!(
                "load_wait_timeout bound={} expected={nodes}",
                self.replies.bound.load(SeqCst)
            ));
            return Err(self.gave_up("loading").into());
        }
        for (deployment, stage) in places(fleet) {
            let stream = self.stream(&self.route(&format!("load-{deployment}-{stage}")));
            self.trace(format!(
                "load_reply deployment={deployment} stage={stage} bound={} failed={:?}",
                stream.bound, stream.failed
            ));
            if let Some(detail) = stream.failed {
                return Err(
                    format!("deployment {deployment} stage {stage} load failed: {detail}").into(),
                );
            }
            if !stream.bound {
                // The backend's own words. A driver that reported only "never
                // bound" made every load failure look the same, which is the
                // one thing an operator cannot work from.
                return Err(format!(
                    "deployment {deployment} stage {stage} never bound: {}",
                    stream.failed.unwrap_or_else(|| "no answer".into())
                )
                .into());
            }
        }
        Ok(())
    }

    pub async fn unload(&self, fleet: &Fleet) -> Result<(), Box<dyn std::error::Error>> {
        let nodes = fleet.deployments().len() * fleet.stages();
        let before = self.replies.released.load(SeqCst);
        for (deployment, stage) in places(fleet) {
            let address = fleet.deployments()[deployment][stage].clone();
            let node = fleet.node_of(deployment, stage);
            let chain = Chain::new(vec![Link {
                address: address.clone(),
                node: node.clone(),
                binding: "deployment".into(),
                generation: 1,
            }])?;
            self.send(
                address,
                Recipient::node(node),
                QueueClass::Control,
                &self.route(&format!("unload-{deployment}-{stage}")),
                Some(chain),
                encode_to_node(&ToNode::Unload),
            )?;
        }
        if !self
            .until(|| self.replies.released.load(SeqCst) >= before + nodes)
            .await
        {
            return Err(self.gave_up("unloading").into());
        }
        Ok(())
    }

    pub async fn save_kv(
        &self,
        fleet: &Fleet,
        sequence: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.cache_all(fleet, sequence, CacheVerb::Save).await
    }

    pub async fn restore_kv(
        &self,
        fleet: &Fleet,
        sequence: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.cache_all(fleet, sequence, CacheVerb::Restore).await
    }

    pub async fn drop_kv(
        &self,
        fleet: &Fleet,
        sequence: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.cache_all(fleet, sequence, CacheVerb::Drop).await
    }

    async fn cache_all(
        &self,
        fleet: &Fleet,
        sequence: &str,
        verb: CacheVerb,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _transaction_guard = self.cache_transaction_lock.lock().await;
        if sequence.is_empty() {
            return Err("KV operation requires a non-empty sequence identity".into());
        }
        let action = verb.route_name();
        let generations: HashMap<_, _> = places(fleet)
            .map(|(deployment, stage)| {
                let route = self.route(&format!("load-{deployment}-{stage}"));
                let generation = self
                    .stream(&route)
                    .bound_generation
                    .ok_or_else(|| format!("KV {action} has no bound generation for {route}"))?;
                Ok(((deployment, stage), generation))
            })
            .collect::<Result<HashMap<_, _>, String>>()
            .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
        let stages: Vec<_> = places(fleet)
            .map(|(deployment, stage)| fleet.node_of(deployment, stage))
            .collect();
        let Some(&generation) = generations.values().next() else {
            return Err("KV operation requires at least one stage".into());
        };
        if generations.values().any(|value| *value != generation) {
            return Err(
                "KV transaction cannot span stages with different bound generations".into(),
            );
        }
        let fleet_identity = fleet_identity(fleet);
        let operation_id = format!("p4-drive-cache-v1-{action}-{fleet_identity}-{sequence}");
        let journal = self.cache_state_dir.join(format!(
            "{action}-{fleet_identity}-{}.journal",
            journal_component(sequence)
        ));
        let kind = match verb {
            CacheVerb::Save => CacheTransactionKind::Persist,
            CacheVerb::Restore => CacheTransactionKind::Restore,
            CacheVerb::Drop => CacheTransactionKind::Discard,
        };
        let mut transaction = match CacheTransaction::recover(&journal) {
            Ok(Some(transaction)) => transaction,
            Ok(None) => CacheTransaction::new(
                operation_id.clone(),
                sequence,
                "deployment",
                generation,
                stages.clone(),
                kind,
            )?
            .with_journal(journal.clone())?,
            Err(error) => {
                return Err(format!("cannot recover {}: {error}", journal.display()).into());
            }
        };
        if transaction.operation_id() != operation_id
            || transaction.sequence() != sequence
            || transaction.kind() != kind
            || transaction.generation() != generation
            || transaction.stages() != stages.as_slice()
        {
            return Err(format!(
                "KV durable transaction identity does not match current fleet: {}",
                journal.display()
            )
            .into());
        }
        if transaction.is_terminal() {
            if transaction.state() == CacheTransactionState::Failed {
                return Err(format!(
                    "KV {action} has a failed durable transaction; inspect {}",
                    journal.display()
                )
                .into());
            }
            self.reconcile_completed_cache_transaction(fleet, &transaction, &generations)
                .await?;
            let _ = std::fs::remove_file(&journal);
            // A completed logical operation is idempotent. Do not issue a
            // second mutation merely because the outer retried after the
            // success record was written.
            return Ok(());
        }
        let attempt = self
            .cache_attempt
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.run_cache_transaction(fleet, &mut transaction, &generations, attempt)
            .await?;
        let _ = std::fs::remove_file(journal);
        Ok(())
    }

    async fn run_cache_transaction(
        &self,
        fleet: &Fleet,
        transaction: &mut CacheTransaction,
        generations: &HashMap<(usize, usize), u64>,
        attempt: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut wave = transaction.start();
        while !wave.is_empty() {
            let mut sent = Vec::with_capacity(wave.len());
            for command in wave {
                let Some((deployment, stage)) = places(fleet).find(|(deployment, stage)| {
                    fleet.node_of(*deployment, *stage) == command.stage
                }) else {
                    return Err(
                        format!("cache command names unknown stage {}", command.stage).into(),
                    );
                };
                let address = fleet.deployments()[deployment][stage].clone();
                let node = command.stage.clone();
                let chain = Chain::new(vec![Link {
                    address: address.clone(),
                    node: node.clone(),
                    binding: "deployment".into(),
                    generation: generations[&(deployment, stage)],
                }])?;
                let route = self.route(&format!(
                    "cache-{}-{}-{attempt}",
                    command.phase.route_name(),
                    command.stage
                ));
                self.send_with_request_id(super::OutboundRequest {
                    target: address,
                    recipient: Recipient::node(node),
                    lane: QueueClass::Control,
                    route: &route,
                    request_id: transaction.operation_id(),
                    chain: Some(chain),
                    body: encode_to_node(&command.body),
                })?;
                sent.push((command, route));
            }
            if !self
                .until(|| {
                    sent.iter()
                        .all(|(_, route)| self.replies.has_cache_reply(route))
                })
                .await
            {
                return Err(self.gave_up("KV transaction wave").into());
            }
            let mut next = Vec::new();
            for (command, route) in sent {
                let reply = self
                    .replies
                    .take_cache_reply(&route)
                    .ok_or_else(|| format!("cache reply disappeared for {route}"))?;
                next.extend(transaction.observe_command(&command, &reply)?);
            }
            wave = next;
        }
        match transaction.state() {
            CacheTransactionState::Complete => Ok(()),
            CacheTransactionState::Failed => Err("KV transaction entered failed state".into()),
            state => Err(format!("KV transaction stopped in non-terminal state {state:?}").into()),
        }
    }
}

fn journal_component(value: &str) -> String {
    value.bytes().map(|byte| format!("{byte:02x}")).collect()
}

fn fleet_identity(fleet: &Fleet) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for (deployment, stages) in fleet.deployments().iter().enumerate() {
        for (stage, address) in stages.iter().enumerate() {
            for byte in format!("{deployment}:{stage}:{address};").bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
    }
    format!("{hash:016x}")
}

#[derive(Clone, Copy)]
enum CacheVerb {
    Save,
    Restore,
    Drop,
}

impl CacheVerb {
    fn route_name(self) -> &'static str {
        match self {
            Self::Save => "save",
            Self::Restore => "restore",
            Self::Drop => "drop",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_components_cannot_escape_the_state_directory() {
        assert_eq!(
            journal_component("sequence/with\\separators"),
            "73657175656e63652f776974685c736570617261746f7273"
        );
    }

    #[test]
    fn fleet_identity_changes_when_a_stage_address_changes() {
        let first = Fleet::parse("127.0.0.1:52001").unwrap();
        let second = Fleet::parse("127.0.0.1:52002").unwrap();
        assert_ne!(fleet_identity(&first), fleet_identity(&second));
    }

    #[test]
    fn drive_uses_the_common_discard_transaction_kind() {
        let mut transaction = CacheTransaction::new(
            "operation",
            "sequence",
            "deployment",
            1,
            ["n0"],
            CacheTransactionKind::Discard,
        )
        .unwrap();
        assert!(matches!(
            transaction.start()[0].body,
            ToNode::PrepareDiscard { .. }
        ));
    }
}

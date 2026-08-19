use super::Session;
use super::deploy::places;
use crate::fleet::Fleet;
use p4_protocol::{Chain, Link, QueueClass, Recipient};
use p4_service::cache::CacheTransaction;
use p4_service::message::wire::encode_to_node;
use p4_service::message::{Reply, ToNode};
use std::collections::HashMap;
use std::sync::atomic::Ordering::SeqCst;

impl Session {
    pub(super) async fn reconcile_completed_cache_transaction(
        &self,
        fleet: &Fleet,
        transaction: &CacheTransaction,
        generations: &HashMap<(usize, usize), u64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let attempt = self.cache_attempt.fetch_add(1, SeqCst);
        let mut sent = Vec::with_capacity(transaction.stages().len());
        for stage_name in transaction.stages() {
            let Some((deployment, stage)) = places(fleet)
                .find(|(deployment, stage)| fleet.node_of(*deployment, *stage) == *stage_name)
            else {
                return Err(
                    format!("completed KV transaction names unknown stage {stage_name}").into(),
                );
            };
            let address = fleet.deployments()[deployment][stage].clone();
            let node = stage_name.clone();
            let generation = generations[&(deployment, stage)];
            let chain = Chain::new(vec![Link {
                address: address.clone(),
                node: node.clone(),
                binding: "deployment".into(),
                generation,
            }])?;
            let route = self.route(&format!("cache-reconcile-{stage_name}-{attempt}"));
            self.send_with_request_id(super::OutboundRequest {
                target: address,
                recipient: Recipient::node(node),
                lane: QueueClass::Control,
                route: &route,
                request_id: transaction.operation_id(),
                chain: Some(chain),
                body: encode_to_node(&ToNode::Reconcile {
                    sequence: transaction.sequence().to_owned(),
                }),
            })?;
            sent.push((stage_name.clone(), generation, route));
        }
        if !self
            .until(|| {
                sent.iter()
                    .all(|(_, _, route)| self.replies.has_cache_reply(route))
            })
            .await
        {
            return Err(self.gave_up("KV receipt reconciliation").into());
        }
        for (stage_name, generation, route) in sent {
            let reply = self
                .replies
                .take_cache_reply(&route)
                .ok_or_else(|| format!("reconcile reply disappeared for {route}"))?;
            validate_reconcile_reply(&stage_name, generation, transaction, &reply)?;
        }
        Ok(())
    }
}

fn validate_reconcile_reply(
    stage_name: &str,
    generation: u64,
    transaction: &CacheTransaction,
    reply: &Reply,
) -> Result<(), Box<dyn std::error::Error>> {
    match reply {
        Reply::CacheStatus {
            stage_id,
            generation: reply_generation,
            operation_id,
            sequence,
            state,
            ..
        } if stage_id == stage_name
            && *reply_generation == generation
            && operation_id == transaction.operation_id()
            && sequence == transaction.sequence()
            && state == "committed" =>
        {
            Ok(())
        }
        Reply::CacheStatus { state, .. } => {
            Err(format!("completed KV receipt for {stage_name} is not committed: {state}").into())
        }
        other => Err(format!("completed KV reconcile for {stage_name} returned {other:?}").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p4_service::cache::CacheTransactionKind;

    fn transaction() -> CacheTransaction {
        CacheTransaction::new(
            "operation",
            "sequence",
            "deployment",
            3,
            ["stage-0"],
            CacheTransactionKind::Persist,
        )
        .unwrap()
    }

    fn receipt(stage: &str, operation: &str, sequence: &str, state: &str) -> Reply {
        Reply::CacheStatus {
            deployment: "deployment".into(),
            stage_id: stage.into(),
            generation: 3,
            operation_id: operation.into(),
            sequence: sequence.into(),
            state: state.into(),
            bytes: 1,
            detail: String::new(),
        }
    }

    #[test]
    fn committed_receipt_with_matching_identity_is_accepted() {
        assert!(
            validate_reconcile_reply(
                "stage-0",
                3,
                &transaction(),
                &receipt("stage-0", "operation", "sequence", "committed"),
            )
            .is_ok()
        );
    }

    #[test]
    fn receipt_identity_mismatch_is_rejected() {
        for (stage, operation, sequence) in [
            ("stage-1", "operation", "sequence"),
            ("stage-0", "other-operation", "sequence"),
            ("stage-0", "operation", "other-sequence"),
        ] {
            assert!(
                validate_reconcile_reply(
                    "stage-0",
                    3,
                    &transaction(),
                    &receipt(stage, operation, sequence, "committed"),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn non_committed_receipt_is_rejected() {
        assert!(
            validate_reconcile_reply(
                "stage-0",
                3,
                &transaction(),
                &receipt("stage-0", "operation", "sequence", "inconsistent"),
            )
            .is_err()
        );
    }
}

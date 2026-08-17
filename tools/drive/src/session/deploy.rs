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
use p4_service::message::wire::{encode_to_agent, encode_to_node};
use p4_service::message::{ToAgent, ToNode};
use std::sync::atomic::Ordering::SeqCst;

/// Every (deployment, stage) pair in the fleet, in the order they are stood up.
fn places(fleet: &Fleet) -> impl Iterator<Item = (usize, usize)> + '_ {
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
            let single = Chain::new(vec![Link {
                address: address.clone(),
                node: node.clone(),
                binding: "deployment".into(),
                generation: 1,
            }])?;
            self.send(
                address,
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
                    artifact: "model".into(),
                    ceiling,
                }),
            )?;
        }
        if !self
            .until(|| self.replies.bound.load(SeqCst) >= nodes)
            .await
        {
            return Err(self.gave_up("loading").into());
        }
        for (deployment, stage) in places(fleet) {
            let stream = self.stream(&self.route(&format!("load-{deployment}-{stage}")));
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
}

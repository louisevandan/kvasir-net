//! Standing a deployment up: a node per stage, then a load on each.
//!
//! Separate from running work on it, because the two change for different
//! reasons — this follows the lifecycle vocabulary and what a load is allowed
//! to report, while its parent follows what an inference does. It is also the
//! half that must fail loudly: a run against a deployment that never bound
//! would otherwise look like a deployment that produced nothing.

use super::Session;
use p4_protocol::{Address, Chain, Link, QueueClass, Recipient};
use p4_service::message::wire::{encode_to_agent, encode_to_node};
use p4_service::message::{ToAgent, ToNode};
use std::sync::atomic::Ordering::SeqCst;

impl Session {
    pub async fn create_nodes(
        &self,
        chain: &[Address],
        adapter: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (stage, address) in chain.iter().enumerate() {
            self.send(
                address.clone(),
                Recipient::Agent,
                QueueClass::Control,
                &self.route(&format!("create-{stage}")),
                None,
                encode_to_agent(&ToAgent::CreateNode {
                    node: Self::node_of(stage, chain.len()),
                    adapter: adapter.to_owned(),
                }),
            )?;
        }
        if !self
            .until(|| self.replies.accepted.load(SeqCst) >= chain.len())
            .await
        {
            return Err(self.gave_up("creating nodes").into());
        }
        for stage in 0..chain.len() {
            let stream = self.stream(&self.route(&format!("create-{stage}")));
            if !stream.accepted {
                return Err(format!(
                    "stage {stage} refused the node: {}",
                    stream.failed.unwrap_or_else(|| "no answer".into())
                )
                .into());
            }
        }
        Ok(())
    }

    pub async fn load(
        &self,
        chain: &[Address],
        ceiling: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (stage, address) in chain.iter().enumerate() {
            let single = Chain::new(vec![Link {
                address: address.clone(),
                node: Self::node_of(stage, chain.len()),
                binding: "deployment".into(),
                generation: 1,
            }])?;
            self.send(
                address.clone(),
                Recipient::node(Self::node_of(stage, chain.len())),
                QueueClass::Control,
                &self.route(&format!("load-{stage}")),
                Some(single),
                encode_to_node(&ToNode::Load {
                    // A mock ignores this; a concrete backend reads it and is
                    // the only thing that knows what it means. The driver
                    // carries it rather than inventing one, because a plan is
                    // opaque above the adapter and inventing one here would be
                    // this tool knowing a backend.
                    plan: self.plans[stage].clone(),
                    artifact: "model".into(),
                    ceiling,
                }),
            )?;
        }
        if !self
            .until(|| self.replies.bound.load(SeqCst) >= chain.len())
            .await
        {
            return Err(self.gave_up("loading").into());
        }
        for stage in 0..chain.len() {
            let stream = self.stream(&self.route(&format!("load-{stage}")));
            if !stream.bound {
                // The backend's own words. A driver that reported only "never
                // bound" made every load failure look the same, which is the
                // one thing an operator cannot work from.
                return Err(format!(
                    "stage {stage} never bound: {}",
                    stream.failed.unwrap_or_else(|| "no answer".into())
                )
                .into());
            }
        }
        Ok(())
    }
}

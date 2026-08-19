//! Discovery preflight for the OUTER driver.

use super::Session;
use super::replies::correlation_key_for_route;
use crate::fleet::Fleet;
use p4_protocol::{QueueClass, Recipient};
use p4_service::message::wire::encode_to_agent;
use p4_service::message::{Reply, ToAgent};
use std::collections::HashMap;

impl Session {
    /// Ask every selected agent for its local model profile before any node is
    /// created. The driver does not interpret the profile; it only verifies
    /// that every response arrived and that the opaque profile bytes agree.
    pub async fn inspect_models(
        &self,
        fleet: &Fleet,
        adapter: &str,
        artifact: &str,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let agents = fleet.addresses();
        let before = self.replies.models.lock().expect("model lock").len();
        for (index, address) in agents.iter().enumerate() {
            self.send(
                address.clone(),
                Recipient::Agent,
                QueueClass::Control,
                &self.route(&format!("discover-{index}")),
                None,
                encode_to_agent(&ToAgent::InspectModel {
                    artifact: artifact.to_owned(),
                    adapter: adapter.to_owned(),
                }),
            )?;
        }
        let expected = agents.len();
        let complete = self
            .until(|| self.replies.models.lock().expect("model lock").len() >= before + expected)
            .await;
        if !complete {
            return Err(self.gave_up("model discovery").into());
        }
        let models = self.replies.models.lock().expect("model lock");
        let mut snapshots = HashMap::new();
        let mut profiles = Vec::new();
        for (index, address) in agents.iter().enumerate() {
            let route = self.route(&format!("discover-{index}"));
            // Model replies are correlated by the same opaque return channel
            // that was put on InspectModel. The agent address is only the
            // transport destination; using it here loses every reply when
            // OUTER advertises a different reachable address.
            let key = correlation_key_for_route(&route, &self.return_channel);
            let Some(Reply::Model {
                artifact: returned_artifact,
                profile,
                capability_snapshot_id,
                expires_at,
                ..
            }) = models.get(&key)
            else {
                return Err(format!("agent {address} returned no model profile").into());
            };
            if returned_artifact != artifact || capability_snapshot_id.is_empty() {
                return Err(format!(
                    "discovery binding mismatch for {address}: artifact or snapshot is invalid"
                )
                .into());
            }
            if !Session::capability_is_valid(
                capability_snapshot_id,
                *expires_at,
                Session::unix_ms(),
            ) {
                return Err(
                    format!("agent {address} returned an expired capability snapshot").into(),
                );
            }
            profiles.push(profile.clone());
            snapshots.insert(
                address.to_string(),
                (capability_snapshot_id.clone(), *expires_at),
            );
        }
        if profiles.windows(2).any(|pair| pair[0] != pair[1]) {
            return Err("discovery profiles disagree across agents".into());
        }
        drop(models);
        self.bind_discovery(artifact.to_owned(), snapshots);
        Ok(profiles.len())
    }
}

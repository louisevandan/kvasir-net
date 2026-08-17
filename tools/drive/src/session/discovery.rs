//! Discovery preflight for the OUTER driver.

use super::Session;
use crate::fleet::Fleet;
use p4_protocol::{QueueClass, Recipient};
use p4_service::message::wire::encode_to_agent;
use p4_service::message::{Reply, ToAgent};

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
        let profiles: Vec<&str> = models
            .values()
            .filter_map(|reply| match reply {
                Reply::Model { profile, .. } => Some(profile.as_str()),
                _ => None,
            })
            .collect();
        if profiles.windows(2).any(|pair| pair[0] != pair[1]) {
            return Err("discovery profiles disagree across agents".into());
        }
        Ok(profiles.len())
    }
}

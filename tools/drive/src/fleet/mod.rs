//! One deployment, or several of the same shape.
//!
//! A chain is one deployment: a model spread over some stages, in order. A
//! fleet with more than one card's worth of room can hold that deployment more
//! than once, and then requests should be spread across the copies rather than
//! queued behind one of them. The driver used to send every request down one
//! chain, so measuring two deployments meant running two drivers and adding
//! their numbers up afterwards — which measures two runs rather than one fleet,
//! and cannot show either one starving the other.
//!
//! ## The grammar
//!
//! `,` separates stages and `;` separates deployments, so a single chain is
//! written exactly as it always was:
//!
//! ```text
//! A:52001,B:52001              one deployment over two machines
//! A:52001,B:52001;C:52001,D:52001   the same deployment twice
//! ```
//!
//! Replicas must have the same number of stages. They are copies of one
//! deployment — that is what makes spreading requests across them meaningful —
//! and if they differed then `P4_DRIVE_SERVE`, which names stage indices, would
//! mean something different for each and silently address the wrong node.

use p4_protocol::Address;

#[derive(Debug)]
pub struct Fleet {
    deployments: Vec<Vec<Address>>,
}

impl Fleet {
    /// Reads the chain argument. `;` between deployments, `,` between stages.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut deployments = Vec::new();
        for chain in text.split(';').filter(|part| !part.trim().is_empty()) {
            let mut stages = Vec::new();
            for stage in chain.split(',').filter(|part| !part.trim().is_empty()) {
                stages.push(
                    format!("tcp://{}", stage.trim())
                        .parse::<Address>()
                        .map_err(|error| format!("{stage} is not an address: {error}"))?,
                );
            }
            if stages.is_empty() {
                return Err("a deployment must name at least one stage".into());
            }
            deployments.push(stages);
        }
        if deployments.is_empty() {
            return Err("a fleet must name at least one deployment".into());
        }
        let stages = deployments[0].len();
        if let Some(odd) = deployments.iter().position(|one| one.len() != stages) {
            return Err(format!(
                "deployment {odd} has {} stages and the first has {stages}: replicas \
                 are copies of one deployment, and a stage index means the same \
                 thing in each",
                deployments[odd].len()
            ));
        }
        Ok(Self { deployments })
    }

    pub fn deployments(&self) -> &[Vec<Address>] {
        &self.deployments
    }

    /// How many stages each deployment has. The same for all of them.
    pub fn stages(&self) -> usize {
        self.deployments[0].len()
    }

    /// Every address in the fleet, once, in the order first seen.
    ///
    /// Used for watching: a share that serves nothing still has a node with a
    /// queue, and "nothing ever queued there" is a claim worth being able to
    /// make. Deduplicated because two deployments may share a machine.
    pub fn addresses(&self) -> Vec<Address> {
        let mut seen: Vec<Address> = Vec::new();
        for stage in self.deployments.iter().flatten() {
            if !seen.contains(stage) {
                seen.push(stage.clone());
            }
        }
        seen
    }

    /// Names one node per stage of one deployment.
    ///
    /// A staged backend reads its position from the name, so this is part of
    /// the placement rather than cosmetic. The replica is in the name only when
    /// there is more than one, which keeps every existing deployment naming its
    /// nodes exactly as before — and it has to be in the name when there is,
    /// because two deployments can share an agent. A machine with two cards is
    /// the obvious case, and without the suffix the second `CreateNode` would
    /// name the node the first one already made.
    pub fn node_of(&self, deployment: usize, stage: usize) -> String {
        let role = match stage + 1 == self.stages() {
            true => "tail",
            false => "stage",
        };
        match self.deployments.len() {
            1 => format!("{role}-{stage}"),
            _ => format!("{role}-{stage}-d{deployment}"),
        }
    }

    /// The plan each load carries, per deployment and stage.
    ///
    /// `P4_DRIVE_PLAN_<d>_<s>` first, then `P4_DRIVE_PLAN_<s>`, then
    /// `P4_DRIVE_PLAN`. Replicas need the per-deployment form because they are
    /// copies in shape and not in placement: two of them on one machine sit on
    /// different cards, and a plan naming a device is the thing that says so.
    pub fn plans(&self, fallback: &str) -> Vec<Vec<String>> {
        (0..self.deployments.len())
            .map(|deployment| {
                (0..self.stages())
                    .map(|stage| {
                        std::env::var(format!("P4_DRIVE_PLAN_{deployment}_{stage}"))
                            .or_else(|_| std::env::var(format!("P4_DRIVE_PLAN_{stage}")))
                            .unwrap_or_else(|_| fallback.to_owned())
                    })
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;

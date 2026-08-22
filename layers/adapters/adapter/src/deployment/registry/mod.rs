//! What lets a P4 broker call `Client::try_submit`/`cancel` without itself
//! knowing which backend a `DeploymentId` names.
//!
//! `SEALED-CONTRACT.md` §9.1 lists exactly this as what stays in P4: "select
//! an adapter instance by `DeploymentId`" and "relay `Submit`/`Cancel` to
//! the adapter's bounded queue." `Registry` is that selection, and nothing
//! more -- it holds one `Arc<dyn Client>` per running deployment and
//! dispatches by `deployment_id`. It does not construct a client, does not
//! know `p4-llamacpp-deployment` exists, and does not touch a socket; those
//! stay behind `Client::try_submit` exactly as `deployment/mod.rs`'s own doc
//! says. This is the type a P4-level test drives to prove a real request
//! reaches a real backend through code that is actually P4's, not a
//! backend's own unit test calling its client directly.

use super::{Client, DeploymentId, EnqueueError, Submit};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Why `Registry::try_submit`/`cancel` could not reach a client at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchError {
    /// No client is registered under this `Submit`'s `deployment_id` --
    /// distinct from `Rejected { DeploymentClosed }`, which is a backend's
    /// own answer about a deployment it *does* know about.
    UnknownDeployment,
    /// The client itself could not enqueue the submission; see
    /// `Client::try_submit`'s own doc for what this does and does not mean.
    Enqueue(EnqueueError),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownDeployment => write!(f, "no client registered for this deployment_id"),
            Self::Enqueue(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for DispatchError {}

/// One entry per running deployment. `register`/`unregister` are how a
/// deployment's lifecycle -- loaded, unloaded, reloaded under a new
/// generation -- reaches this map; nothing here decides when that happens.
#[derive(Default)]
pub struct Registry {
    clients: RwLock<HashMap<DeploymentId, Arc<dyn Client>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a client for `deployment_id`, replacing whatever was
    /// registered under that id before -- the caller (whatever owns
    /// deployment lifecycle) decides whether a replacement is a reload of
    /// the same deployment or a mistake; this type does not judge that.
    pub fn register(&self, deployment_id: DeploymentId, client: Arc<dyn Client>) {
        self.clients
            .write()
            .expect("registry lock")
            .insert(deployment_id, client);
    }

    /// Removes and returns whatever client was registered for
    /// `deployment_id`, if any.
    pub fn unregister(&self, deployment_id: &str) -> Option<Arc<dyn Client>> {
        self.clients
            .write()
            .expect("registry lock")
            .remove(deployment_id)
    }

    pub fn contains(&self, deployment_id: &str) -> bool {
        self.clients
            .read()
            .expect("registry lock")
            .contains_key(deployment_id)
    }

    /// Looks up `submit.deployment_id` and forwards to that client's own
    /// `try_submit` -- the whole of what a P4 broker does with a `Submit`
    /// per `SEALED-CONTRACT.md` §9.1/§9.4. No admission judgement happens
    /// here: `UnknownDeployment` is the only new outcome this adds, and it
    /// means exactly what it says -- no client is registered, not that one
    /// refused the work.
    pub fn try_submit(&self, submit: Submit) -> Result<(), DispatchError> {
        let client = {
            let clients = self.clients.read().expect("registry lock");
            clients.get(&submit.deployment_id).cloned()
        };
        match client {
            Some(client) => client.try_submit(submit).map_err(DispatchError::Enqueue),
            None => Err(DispatchError::UnknownDeployment),
        }
    }

    /// Forwards a cancel to `deployment_id`'s client, or no-ops if none is
    /// registered -- consistent with `Client::cancel` itself treating an
    /// unknown `submission_id` as a no-op rather than an error. Returns
    /// whether a client was actually found, for a caller that wants to log
    /// the difference; nothing here requires checking it.
    pub fn cancel(&self, deployment_id: &str, submission_id: super::SubmissionId) -> bool {
        let client = {
            let clients = self.clients.read().expect("registry lock");
            clients.get(deployment_id).cloned()
        };
        match client {
            Some(client) => {
                client.cancel(submission_id);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests;

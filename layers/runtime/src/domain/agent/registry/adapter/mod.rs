//! Adapter records and the rules that guard their registration.
//!
//! Adapters self-register; a controller never supplies an endpoint. This is
//! the only place a routable concrete runtime handle enters the Agent, so
//! re-registration is guarded rather than blindly overwritten.

use crate::foundation::transport::SharedTransport;

#[derive(Clone)]
pub(crate) struct Adapter {
    pub(crate) kind: String,
    pub(crate) transport: SharedTransport,
    /// `None` for a co-resident in-process handler, which has no wire address.
    pub(crate) endpoint: Option<String>,
    pub(crate) descriptor: String,
}

/// The transport is a live handle with no useful representation; the routable
/// identity is the endpoint.
impl std::fmt::Debug for Adapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Adapter")
            .field("kind", &self.kind)
            .field("endpoint", &self.endpoint)
            .field("descriptor", &self.descriptor)
            .finish_non_exhaustive()
    }
}

/// Why a registration was refused.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RegistrationRefusal {
    /// ID, kind, or endpoint was missing or unresolvable.
    Malformed,
    /// The adapter ID already routes attached NodeSlots to another endpoint.
    /// Accepting it would silently redirect their traffic.
    EndpointReboundWhileAttached {
        registered: String,
        attached_nodes: usize,
    },
}

impl RegistrationRefusal {
    pub(crate) fn detail(&self) -> String {
        match self {
            Self::Malformed => {
                "adapter registration needs ID, kind, and valid endpoint".into()
            }
            Self::EndpointReboundWhileAttached {
                registered,
                attached_nodes,
            } => format!(
                "adapter is already registered at {registered} with {attached_nodes} attached node(s); \
                 unload and remove them before moving the adapter endpoint"
            ),
        }
    }
}

/// Decides whether an inbound `ADAPTER_REGISTER` may replace what is stored.
///
/// A repeated registration from the same endpoint is idempotent: adapters
/// re-register after their own restart and must keep serving their slots.
/// Changing the endpoint while slots are attached is refused, because those
/// slots were authorised against the previously registered runtime.
pub(crate) fn authorize_registration(
    existing: Option<&Adapter>,
    endpoint: &str,
    attached_nodes: usize,
) -> Result<(), RegistrationRefusal> {
    let Some(existing) = existing else {
        return Ok(());
    };
    let registered = existing.endpoint.as_deref().unwrap_or("in-memory");
    if registered == endpoint || attached_nodes == 0 {
        return Ok(());
    }
    Err(RegistrationRefusal::EndpointReboundWhileAttached {
        registered: registered.into(),
        attached_nodes,
    })
}

#[cfg(test)]
mod tests;

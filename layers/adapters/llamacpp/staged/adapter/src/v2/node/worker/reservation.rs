use super::*;

impl Worker {
    /// Reserve the complete retained result group before native execution.
    /// Legacy direct fixtures have no byte budget and continue through the old
    /// publication path; production owned workers must use the bounded store.
    pub(super) fn reserve_native_completion_pool(
        &mut self,
        count: usize,
    ) -> Result<Option<CompletionReservationGroup>, String> {
        if !self.owned_completions {
            return Ok(None);
        }
        let profile = self
            .state
            .resource_profile
            .as_ref()
            .ok_or("native completion reservation requires a resource profile")?;
        let retained_bytes = usize::try_from(profile.max_completion_retained_bytes)
            .map_err(|_| "native completion retained bytes exceed platform range")?;
        loop {
            match self.publisher.try_reserve_pool(count, retained_bytes) {
                Ok(group) => return Ok(Some(group)),
                Err(GroupReserveError::Full) => {
                    if self.shutting_down.load(Ordering::SeqCst) {
                        return Err("native completion reservation stopped at shutdown".into());
                    }
                    self.set_snapshot("native_completion_reservation_full:waiting");
                    self.service_blocked_ack()
                        .map_err(|_| "native completion reservation ACK service failed")?;
                    std::thread::sleep(COMPLETION_RETRY_INTERVAL);
                }
                Err(error) => {
                    return Err(format!("native completion reservation failed: {error:?}"));
                }
            }
        }
    }
}

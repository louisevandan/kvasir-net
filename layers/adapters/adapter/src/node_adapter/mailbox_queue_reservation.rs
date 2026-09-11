//! Synchronous queue-slot reservations for atomic broker forwarding. These
//! reserve delivery slots, separately from retained count/byte permissions.
use super::*;

pub struct CompletionQueueReservation {
    storage: Arc<Mutex<Storage>>,
    capacity: Arc<Mutex<CapacityState>>,
    active: bool,
}

impl std::fmt::Debug for CompletionQueueReservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompletionQueueReservation")
            .field("active", &self.active)
            .finish()
    }
}

impl Drop for CompletionQueueReservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        {
            let mut storage = self.storage.lock().unwrap_or_else(|e| e.into_inner());
            storage.reserved_slots = storage
                .reserved_slots
                .checked_sub(1)
                .expect("slot is owned");
        }
        self.active = false;
        if !std::thread::panicking() {
            notify_capacity(&self.capacity, false);
        }
    }
}

#[derive(Debug)]
pub struct QueuePublishError {
    pub event: Event,
    pub reservation: CompletionReservation,
    pub slot: CompletionQueueReservation,
    pub reason: ReservedPublishReason,
}

#[derive(Debug)]
pub struct RetainedQueueTransferError {
    pub completion: RetainedCompletion,
    pub reservation: CompletionReservation,
    pub slot: CompletionQueueReservation,
    pub reason: ReservedPublishReason,
}

impl CompletionPublisher {
    pub fn same_mailbox(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.receiver, &other.receiver)
    }

    /// Atomically reserve immediate delivery and its retained storage. A
    /// refused admission creates no temporary claim and sends no self-wakeup.
    pub fn try_reserve_delivery(
        &self,
        event_bytes: usize,
    ) -> Result<(CompletionQueueReservation, CompletionReservation), ReserveError> {
        let bytes = event_bytes
            .checked_add(COMPLETION_ENTRY_OVERHEAD_BYTES)
            .ok_or(ReserveError::CostOverflow)?;
        let mut storage = self.receiver.lock().map_err(|_| ReserveError::Closed)?;
        if storage.closed {
            return Err(ReserveError::Closed);
        }
        {
            let budget = self.budget.lock().map_err(|_| ReserveError::Closed)?;
            if budget.closed {
                return Err(ReserveError::Closed);
            }
            if let Some(limit) = budget.byte_limit.filter(|&limit| bytes > limit) {
                return Err(ReserveError::TooLarge {
                    required: bytes,
                    limit,
                });
            }
        }
        if storage.queue.len() + storage.reserved_slots >= storage.queue_capacity {
            return Err(ReserveError::Full);
        }
        let reservation = self.try_reserve(1, event_bytes)?;
        storage.reserved_slots += 1;
        Ok((
            CompletionQueueReservation {
                storage: Arc::clone(&self.receiver),
                capacity: Arc::clone(&self.capacity),
                active: true,
            },
            reservation,
        ))
    }

    /// Both permissions are consumed by the same actual enqueue. No callback
    /// runs here; notification and any refused owners must be handled unlocked.
    pub fn publish_with_queue_deferred(
        &self,
        event: Event,
        reservation: CompletionReservation,
        mut slot: CompletionQueueReservation,
    ) -> Result<DeferredCompletionNotification, QueuePublishError> {
        let reason = if !Arc::ptr_eq(&self.receiver, &slot.storage)
            || !Arc::ptr_eq(&self.budget, &reservation.claim.budget)
        {
            Some(ReservedPublishReason::WrongMailbox)
        } else {
            match retained_event_bytes(&event)
                .ok()
                .and_then(|n| n.checked_add(COMPLETION_ENTRY_OVERHEAD_BYTES))
            {
                None => Some(ReservedPublishReason::CostOverflow),
                Some(required) if required > reservation.claim.bytes => {
                    Some(ReservedPublishReason::TooSmall {
                        required,
                        reserved: reservation.claim.bytes,
                    })
                }
                Some(_) => None,
            }
        };
        if let Some(reason) = reason {
            return Err(QueuePublishError {
                event,
                reservation,
                slot,
                reason,
            });
        }
        let Ok(mut storage) = self.receiver.lock() else {
            return Err(QueuePublishError {
                event,
                reservation,
                slot,
                reason: ReservedPublishReason::Closed,
            });
        };
        if storage.closed {
            return Err(QueuePublishError {
                event,
                reservation,
                slot,
                reason: ReservedPublishReason::Closed,
            });
        }
        storage.reserved_slots = storage
            .reserved_slots
            .checked_sub(1)
            .expect("validated queue slot");
        slot.active = false;
        Ok(self.enqueue_locked(&mut storage, event, reservation, true))
    }
}

impl RetainedCompletion {
    pub fn transfer_with_queue_deferred(
        mut self,
        destination: &CompletionPublisher,
        reservation: CompletionReservation,
        slot: CompletionQueueReservation,
    ) -> Result<DeferredCompletionNotification, RetainedQueueTransferError> {
        let event = self.event.take().expect("retained Event is owned");
        match destination.publish_with_queue_deferred(event, reservation, slot) {
            Ok(mut notification) => {
                notification.source_claim = self.claim.take();
                Ok(notification)
            }
            Err(error) => {
                self.event = Some(error.event);
                Err(RetainedQueueTransferError {
                    completion: self,
                    reservation: error.reservation,
                    slot: error.slot,
                    reason: error.reason,
                })
            }
        }
    }
}

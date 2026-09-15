use crate::construction::{HfNodeAdapter, Input};
use p4_adapter::node_adapter::*;
use std::{
    sync::atomic::Ordering,
    task::{Context, Poll},
};

impl RetainedNodeAdapter for HfNodeAdapter {
    fn try_offer_retained(&self, completion: RetainedCompletion) -> Result<(), RetainedOfferError> {
        if completion.event().validate().is_err() {
            return Err(RetainedOfferError::Closed(completion));
        }
        if self.stop.load(Ordering::Acquire) {
            return Err(RetainedOfferError::Closed(completion));
        }
        let cost = completion.retained_bytes();
        if cost > self.limit {
            return Err(RetainedOfferError::Closed(completion));
        }
        if self
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(cost).filter(|n| *n <= self.limit)
            })
            .is_err()
        {
            return Err(RetainedOfferError::Full(completion));
        }
        self.count.fetch_add(1, Ordering::AcqRel);
        let input = Input {
            completion: Some(completion),
            count: self.count.clone(),
            bytes: self.bytes.clone(),
            cost,
        };
        match self.sender.as_ref().unwrap().try_send(input) {
            Ok(()) => Ok(()),
            Err(error) => {
                let closed = matches!(error, tokio::sync::mpsc::error::TrySendError::Closed(_));
                let mut input = error.into_inner();
                // Move the exact allocation and claim back; Drop still returns our independent byte charge.
                let completion = input.completion.take().unwrap();
                if closed {
                    Err(RetainedOfferError::Closed(completion))
                } else {
                    Err(RetainedOfferError::Full(completion))
                }
            }
        }
    }
    fn peek_retained_completion(&self) -> Option<CompletionFront> {
        self.mailbox.peek_owned_front()
    }
    fn try_take_retained_matching(&self, expected: &CompletionFront) -> OwnedPoll {
        self.mailbox.try_take_owned_matching(expected)
    }
    fn poll_take_retained(&self, cx: &mut Context<'_>) -> Poll<OwnedPoll> {
        self.mailbox.poll_take_owned(cx)
    }
    fn snapshot(&self) -> String {
        let state = self.state.lock().unwrap().clone();
        if state == "uncertain" {
            return state;
        }
        if self.bytes.load(Ordering::Acquire) != 0
            || self.mailbox.storage_snapshot().retained_count != 0
        {
            return "busy".into();
        }
        state
    }
    fn completion_storage_snapshot(&self) -> Option<CompletionStorageSnapshot> {
        Some(self.mailbox.storage_snapshot())
    }
    fn retention_snapshot(&self) -> Option<AdapterRetentionSnapshot> {
        Some(AdapterRetentionSnapshot {
            pending_requests: AdapterRetainedStorage {
                count: self.count.load(Ordering::Acquire),
                bytes: self.bytes.load(Ordering::Acquire),
            },
            native_responses: AdapterRetainedStorage::default(),
        })
    }
}

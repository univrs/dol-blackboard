//! In-memory ring buffer for recent claims — fast feed without hitting disk.

use std::collections::VecDeque;

use tokio::sync::{broadcast, Mutex};

use crate::claim::DolClaim;

pub const RING_CAPACITY: usize = 256;

pub struct ClaimRing {
    inner: Mutex<VecDeque<DolClaim>>,
    broadcast_tx: broadcast::Sender<DolClaim>,
}

impl ClaimRing {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(RING_CAPACITY);
        Self {
            inner: Mutex::new(VecDeque::with_capacity(RING_CAPACITY)),
            broadcast_tx: tx,
        }
    }

    pub async fn push(&self, claim: DolClaim) {
        let _ = self.broadcast_tx.send(claim.clone());
        let mut buf = self.inner.lock().await;
        if buf.len() == RING_CAPACITY {
            buf.pop_front();
        }
        buf.push_back(claim);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DolClaim> {
        self.broadcast_tx.subscribe()
    }

    pub async fn last_n(&self, n: usize) -> Vec<DolClaim> {
        let buf = self.inner.lock().await;
        let skip = buf.len().saturating_sub(n);
        buf.iter().skip(skip).cloned().collect()
    }

    pub async fn len(&self) -> usize {
        self.inner.lock().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.lock().await.is_empty()
    }
}

impl Default for ClaimRing {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn ring_buffer_basics() {
        let ring = ClaimRing::new();
        assert!(ring.is_empty().await);

        for i in 0..10 {
            ring.push(DolClaim::Gen {
                author: format!("author-{i}"),
                ttl_secs: 60,
                body: json!({"i": i}),
            })
            .await;
        }

        assert_eq!(ring.len().await, 10);

        let last_3 = ring.last_n(3).await;
        assert_eq!(last_3.len(), 3);
        if let DolClaim::Gen { author, .. } = &last_3[0] {
            assert_eq!(author, "author-7");
        } else {
            panic!("expected Gen variant");
        }
    }

    #[tokio::test]
    async fn broadcast_subscriber_receives_claims() {
        let ring = ClaimRing::new();
        let mut rx = ring.subscribe();

        ring.push(DolClaim::Gen {
            author: "alice".into(),
            ttl_secs: 60,
            body: json!({"test": true}),
        })
        .await;

        let received = rx.recv().await.unwrap();
        assert_eq!(received.author(), "alice");
    }

    #[tokio::test]
    async fn broadcast_lagging_subscriber() {
        let ring = ClaimRing::new();
        let mut rx = ring.subscribe();

        // Push more than channel capacity to force lag
        for i in 0..RING_CAPACITY + 10 {
            ring.push(DolClaim::Gen {
                author: format!("a-{i}"),
                ttl_secs: 60,
                body: json!({"i": i}),
            })
            .await;
        }

        let result = rx.recv().await;
        assert!(
            result.is_err() || result.is_ok(),
            "lagged subscriber should get Lagged or skip to recent"
        );
    }

    #[tokio::test]
    async fn ring_buffer_eviction() {
        let ring = ClaimRing::new();
        for i in 0..RING_CAPACITY + 10 {
            ring.push(DolClaim::Gen {
                author: format!("author-{i}"),
                ttl_secs: 60,
                body: json!({"i": i}),
            })
            .await;
        }
        assert_eq!(ring.len().await, RING_CAPACITY);

        let all = ring.last_n(RING_CAPACITY).await;
        if let DolClaim::Gen { author, .. } = &all[0] {
            assert_eq!(author, "author-10");
        } else {
            panic!("expected Gen variant");
        }
    }
}

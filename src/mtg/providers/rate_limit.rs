//! A minimal "one request per interval" gate.
//!
//! Callers `acquire().await` immediately before sending a request. The first call
//! returns at once; each later call waits until at least `min_interval` has passed
//! since the previous caller was let through. Nothing waits *after* a request, so a
//! burst of N calls takes `(N - 1) * min_interval`, not `N * min_interval`.
//!
//! Waiters queue on a fair (FIFO) `tokio::sync::Mutex`, so concurrent callers are let
//! through in the order they started waiting. Uses `tokio::time`, so tests can run it
//! under paused time.

use tokio::sync::Mutex;
use tokio::time::{sleep_until, Duration, Instant};

pub struct RateLimiter {
    min_interval: Duration,
    /// Earliest instant the next caller may proceed; `None` until the first call.
    next_allowed: Mutex<Option<Instant>>,
}

impl RateLimiter {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            next_allowed: Mutex::new(None),
        }
    }

    /// Wait until the caller may send its request.
    ///
    /// The lock is held while sleeping so later callers queue behind this one. If the
    /// future is dropped mid-wait the slot is simply not consumed.
    pub async fn acquire(&self) {
        let mut next_allowed = self.next_allowed.lock().await;
        if let Some(at) = *next_allowed {
            sleep_until(at).await;
        }
        *next_allowed = Some(Instant::now() + self.min_interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const SECOND: Duration = Duration::from_secs(1);

    #[tokio::test(start_paused = true)]
    async fn first_acquire_is_immediate() {
        let limiter = RateLimiter::new(SECOND);
        let start = Instant::now();
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn back_to_back_acquires_are_spaced_by_the_interval() {
        let limiter = RateLimiter::new(SECOND);
        let start = Instant::now();
        let mut at = Vec::new();
        for _ in 0..4 {
            limiter.acquire().await;
            at.push(start.elapsed());
        }
        assert_eq!(
            at,
            vec![
                Duration::ZERO,
                SECOND,
                Duration::from_secs(2),
                Duration::from_secs(3)
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn waits_only_for_the_remainder_of_the_interval() {
        let limiter = RateLimiter::new(SECOND);
        limiter.acquire().await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let start = Instant::now();
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::from_millis(700));
    }

    #[tokio::test(start_paused = true)]
    async fn no_wait_once_the_interval_has_passed() {
        let limiter = RateLimiter::new(SECOND);
        limiter.acquire().await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        let start = Instant::now();
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_callers_are_serialized_in_arrival_order() {
        let limiter = Arc::new(RateLimiter::new(SECOND));
        let start = Instant::now();
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        let tasks: Vec<_> = (0..5)
            .map(|i| {
                let limiter = limiter.clone();
                let order = order.clone();
                tokio::spawn(async move {
                    limiter.acquire().await;
                    order.lock().unwrap().push((i, start.elapsed()));
                })
            })
            .collect();
        for task in tasks {
            task.await.unwrap();
        }

        let order = order.lock().unwrap().clone();
        let expected: Vec<_> = (0..5).map(|i| (i, SECOND * i as u32)).collect();
        assert_eq!(order, expected);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_waiter_does_not_consume_a_slot() {
        let limiter = RateLimiter::new(SECOND);
        limiter.acquire().await;
        // Give up on the second acquire half-way through its wait.
        let gave_up = tokio::time::timeout(Duration::from_millis(500), limiter.acquire()).await;
        assert!(gave_up.is_err());
        // The next caller still only waits for the original slot (1s after the first).
        let start = Instant::now();
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::from_millis(500));
    }
}

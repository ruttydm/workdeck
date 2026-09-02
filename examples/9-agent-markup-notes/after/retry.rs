use std::future::Future;
use std::time::Duration;

pub const DEFAULT_ATTEMPTS: usize = 3;

pub async fn fetch_with_retry<T, E, Fetch, FetchFuture, Sleep, SleepFuture>(
    url: &str,
    attempts: usize,
    mut fetch: Fetch,
    mut sleep: Sleep,
) -> Result<T, E>
where
    Fetch: FnMut(&str) -> FetchFuture,
    FetchFuture: Future<Output = Result<T, E>>,
    Sleep: FnMut(Duration) -> SleepFuture,
    SleepFuture: Future<Output = ()>,
{
    let mut delay = Duration::from_millis(100);
    for attempt in 1..=attempts {
        match fetch(url).await {
            Ok(response) => return Ok(response),
            Err(error) => {
                if attempt == attempts {
                    return Err(error);
                }
                sleep(delay).await;
                delay = delay.saturating_mul(2);
            }
        }
    }
    unreachable!("a zero-attempt retry has no response")
}

pub async fn fetch_with_default_retry<T, E, Fetch, FetchFuture, Sleep, SleepFuture>(
    url: &str,
    fetch: Fetch,
    sleep: Sleep,
) -> Result<T, E>
where
    Fetch: FnMut(&str) -> FetchFuture,
    FetchFuture: Future<Output = Result<T, E>>,
    Sleep: FnMut(Duration) -> SleepFuture,
    SleepFuture: Future<Output = ()>,
{
    fetch_with_retry(url, DEFAULT_ATTEMPTS, fetch, sleep).await
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::future::{Future, ready};
    use std::task::{Context, Poll, Waker};

    use super::*;

    fn block_on<F: Future>(future: F) -> F::Output {
        let mut future = Box::pin(future);
        let mut context = Context::from_waker(Waker::noop());
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    #[test]
    fn retries_with_bounded_exponential_backoff() {
        let attempts = Cell::new(0_usize);
        let delays = RefCell::new(Vec::new());
        let response = block_on(fetch_with_default_retry(
            "https://example.test",
            |url| {
                attempts.set(attempts.get() + 1);
                ready(if attempts.get() < 3 {
                    Err("offline")
                } else {
                    Ok(format!("response from {url}"))
                })
            },
            |delay| {
                delays.borrow_mut().push(delay);
                ready(())
            },
        ))
        .expect("third fetch succeeds");

        assert_eq!(response, "response from https://example.test");
        assert_eq!(attempts.get(), 3);
        assert_eq!(
            *delays.borrow(),
            [Duration::from_millis(100), Duration::from_millis(200)]
        );
    }

    #[test]
    fn rethrows_the_last_error_without_an_extra_sleep() {
        let delays = RefCell::new(Vec::new());
        let error = block_on(fetch_with_retry(
            "https://example.test",
            2,
            |_| ready(Err::<(), _>("still offline")),
            |delay| {
                delays.borrow_mut().push(delay);
                ready(())
            },
        ))
        .expect_err("final error is returned");

        assert_eq!(error, "still offline");
        assert_eq!(*delays.borrow(), [Duration::from_millis(100)]);
    }
}

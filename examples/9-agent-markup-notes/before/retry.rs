use std::future::Future;

pub async fn fetch_once<T, E, Fetch, FetchFuture>(url: &str, mut fetch: Fetch) -> Result<T, E>
where
    Fetch: FnMut(&str) -> FetchFuture,
    FetchFuture: Future<Output = Result<T, E>>,
{
    fetch(url).await
}

#[cfg(test)]
mod tests {
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
    fn fetches_exactly_once() {
        let response = block_on(fetch_once("https://example.test", |url| {
            ready(Ok::<_, &'static str>(format!("response from {url}")))
        }))
        .expect("single fetch succeeds");

        assert_eq!(response, "response from https://example.test");
    }
}

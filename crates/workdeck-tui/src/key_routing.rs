//! Central key ownership and focused-widget delivery policy.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOwner {
    NotMine,
    Mine,
    Focused,
}

/// Walk the global handler chain and consume keys claimed by dispatch-owned handlers.
///
/// `Focused` ends global routing without consuming the event so a focused text surface can still
/// receive it. `Mine` both ends routing and consumes it. `NotMine` advances to the next handler.
pub fn route_key_ownership<K>(
    handlers: &mut [&mut dyn FnMut(&K) -> KeyOwner],
    key: &K,
    mut consume: impl FnMut(&K),
) -> bool {
    for handle in handlers {
        match handle(key) {
            KeyOwner::NotMine => {}
            KeyOwner::Mine => {
                consume(key);
                return true;
            }
            KeyOwner::Focused => return true,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestKey(&'static str);

    #[test]
    fn walks_past_not_mine_until_one_handler_owns_and_consumes_the_key() {
        let calls = RefCell::new(Vec::new());
        let mut first = |key: &TestKey| {
            calls.borrow_mut().push(("first", key.clone()));
            KeyOwner::NotMine
        };
        let mut second = |key: &TestKey| {
            calls.borrow_mut().push(("second", key.clone()));
            KeyOwner::Mine
        };
        let mut consumed = Vec::new();
        let key = TestKey("j");
        let owned = route_key_ownership(&mut [&mut first, &mut second], &key, |key| {
            consumed.push(key.clone())
        });

        assert!(owned);
        assert_eq!(
            calls.into_inner(),
            [("first", key.clone()), ("second", key.clone())]
        );
        assert_eq!(consumed, [key]);
    }

    #[test]
    fn mine_stops_the_chain_after_consuming() {
        let reached = RefCell::new(Vec::new());
        let mut owner = |_: &TestKey| {
            reached.borrow_mut().push("owner");
            KeyOwner::Mine
        };
        let mut unreached = |_: &TestKey| {
            reached.borrow_mut().push("unreached");
            KeyOwner::Mine
        };
        let mut consumed = Vec::new();
        let key = TestKey("j");
        assert!(route_key_ownership(
            &mut [&mut owner, &mut unreached],
            &key,
            |key| consumed.push(key.clone()),
        ));
        assert_eq!(reached.into_inner(), ["owner"]);
        assert_eq!(consumed, [key]);
    }

    #[test]
    fn focused_stops_the_chain_without_consuming() {
        let reached = RefCell::new(Vec::new());
        let mut focused = |_: &TestKey| {
            reached.borrow_mut().push("focused");
            KeyOwner::Focused
        };
        let mut unreached = |_: &TestKey| {
            reached.borrow_mut().push("unreached");
            KeyOwner::Mine
        };
        let mut consumed = Vec::new();
        assert!(route_key_ownership(
            &mut [&mut focused, &mut unreached],
            &TestKey("j"),
            |key| consumed.push(key.clone()),
        ));
        assert_eq!(reached.into_inner(), ["focused"]);
        assert!(consumed.is_empty());
    }

    #[test]
    fn no_owner_and_an_empty_chain_return_false_without_consuming() {
        let calls = RefCell::new(0);
        let mut first = |_: &TestKey| {
            *calls.borrow_mut() += 1;
            KeyOwner::NotMine
        };
        let mut second = |_: &TestKey| {
            *calls.borrow_mut() += 1;
            KeyOwner::NotMine
        };
        let mut consumed = Vec::new();
        assert!(!route_key_ownership(
            &mut [&mut first, &mut second],
            &TestKey("j"),
            |key| consumed.push(key.clone()),
        ));
        assert_eq!(calls.into_inner(), 2);
        assert!(consumed.is_empty());

        let mut empty: [&mut dyn FnMut(&TestKey) -> KeyOwner; 0] = [];
        assert!(!route_key_ownership(&mut empty, &TestKey("j"), |_| {}));
    }
}

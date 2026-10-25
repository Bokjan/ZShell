//! Requests for many remote paths at once: a few at a time (each costs a round trip, which one
//! after another add up to minutes on a distant server), and a tree's folders by level.

use std::cmp::Reverse;
use std::future::Future;

use futures_util::{stream, Stream, StreamExt};

/// `f` on each of `items`, up to `limit` at once, the results as they come. The caller decides
/// what an error does: collected with the rest, or ending the stream (`try_collect`), after
/// which the requests still going are dropped.
///
/// By index: closures given references make futures the compiler can't prove `Send`, which
/// a Tauri command's must be.
pub(super) fn each<'a, T, F, Fut>(items: &'a [T], limit: usize, f: F) -> impl Stream<Item = Fut::Output> + 'a
where
    T: Sync,
    F: Fn(&'a T) -> Fut + 'a,
    Fut: Future + 'a,
{
    stream::iter(0..items.len()).map(move |i| f(&items[i])).buffer_unordered(limit)
}

/// [`each`], the results in the order of `items`: one taking long holds the next `limit` back.
pub(super) fn each_in_order<'a, T, F, Fut>(items: &'a [T], limit: usize, f: F) -> impl Stream<Item = Fut::Output> + 'a
where
    T: Sync,
    F: Fn(&'a T) -> Fut + 'a,
    Fut: Future + 'a,
{
    stream::iter(0..items.len()).map(move |i| f(&items[i])).buffered(limit)
}

/// Which end of a tree [`levels`] gives first.
pub(super) enum Order {
    /// Parents before what is in them: to create folders.
    ParentsFirst,
    /// What is in folders before them: to delete them.
    DeepestFirst,
}

/// `items` sorted by the depth of their remote `path` (its `/`s) and split into levels of one
/// depth each, in `order`; those of one level keep their order.
pub(super) fn levels<'a, T, P>(items: &'a mut [T], path: P, order: Order) -> impl Iterator<Item = &'a [T]> + 'a
where
    P: Fn(&T) -> &str + 'a,
{
    let depth = move |item: &T| path(item).matches('/').count();
    match order {
        Order::ParentsFirst => items.sort_by_key(&depth),
        Order::DeepestFirst => items.sort_by_key(|item| Reverse(depth(item))),
    }
    items.chunk_by(move |a, b| depth(a) == depth(b))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn levels_of_a_tree() {
        let mut dirs = ["/a/b", "/a", "/c", "/a/b/d", "/c/e"];
        let parents: Vec<Vec<&str>> = levels(&mut dirs, |dir| dir, Order::ParentsFirst).map(<[_]>::to_vec).collect();
        assert_eq!(parents, [vec!["/a", "/c"], vec!["/a/b", "/c/e"], vec!["/a/b/d"]]);
        let deepest: Vec<Vec<&str>> = levels(&mut dirs, |dir| dir, Order::DeepestFirst).map(<[_]>::to_vec).collect();
        assert_eq!(deepest, [vec!["/a/b/d"], vec!["/a/b", "/c/e"], vec!["/a", "/c"]]);
    }

    /// The later items finish first.
    async fn slower_first(n: &u64) -> u64 {
        tokio::time::sleep(Duration::from_millis(40 - 10 * n)).await;
        *n
    }

    #[tokio::test]
    async fn each_gives_results_as_they_come_or_in_order() {
        let items = [0, 1, 2, 3];
        let came: Vec<u64> = each(&items, 4, slower_first).collect().await;
        assert_eq!(came, [3, 2, 1, 0]);
        let ordered: Vec<u64> = each_in_order(&items, 4, slower_first).collect().await;
        assert_eq!(ordered, [0, 1, 2, 3]);
    }
}

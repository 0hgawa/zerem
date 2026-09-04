//! How many torrents download at once, and which ones.
//!
//! Twenty torrents sharing one line is twenty torrents that never finish. Every
//! client worth using answers this the same way — a limit, and a queue — and
//! librqbit has neither, so the queue is built out of the lever it does have:
//! pausing. Same bargain as fetching one file before the others, and the same
//! honesty about it: a queued torrent really is stopped, and says so.
//!
//! **Seeding is never queued.** A finished torrent costs no download bandwidth,
//! which is the thing being rationed, and stopping it to make room would be
//! taking from the swarm to give to oneself. So the limit counts downloads and
//! only downloads.

use crate::TorrentId;

/// One torrent, as the queue sees it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Waiting {
    pub id: TorrentId,
    /// Where it sits. Lower goes first, and it is signed so that "move to the
    /// front" is a subtraction rather than a renumbering of everything else.
    pub position: i64,
    /// Whether the user wants it running at all. Not the same as whether it is
    /// running: the queue owns that, and this is the intent it works from.
    pub wanted: bool,
    /// A finished torrent seeds, and seeding does not wait in line.
    pub complete: bool,
}

/// Which torrents should be running, given how many may download at once.
///
/// `limit` of zero is no limit, which is what an unlimited setting stores —
/// the same convention the transfer caps use.
///
/// The order is the queue's, so the returned list is also the order they were
/// admitted in. Callers want membership rather than order, but a list is what
/// makes the rule testable by reading it.
#[must_use]
pub fn admit(waiting: &[Waiting], limit: u32) -> Vec<TorrentId> {
    let mut order: Vec<&Waiting> = waiting.iter().filter(|w| w.wanted).collect();
    // By position, then by id: two torrents can share a position only if the
    // counter wrapped, and a stable answer beats a clever one.
    order.sort_unstable_by_key(|w| (w.position, w.id));

    let mut admitted = Vec::with_capacity(order.len());
    let mut downloading = 0_u32;
    for w in order {
        if w.complete {
            // Seeding is free of the limit and free of the queue.
            admitted.push(w.id);
            continue;
        }
        if limit == 0 || downloading < limit {
            downloading += 1;
            admitted.push(w.id);
        }
    }
    admitted
}

#[cfg(test)]
mod tests {
    use super::{admit, Waiting};
    use crate::TorrentId;

    fn at(id: u32, position: i64) -> Waiting {
        Waiting { id: TorrentId(id), position, wanted: true, complete: false }
    }

    fn ids(waiting: &[Waiting], limit: u32) -> Vec<u32> {
        admit(waiting, limit).into_iter().map(|id| id.0).collect()
    }

    #[test]
    fn no_limit_admits_everything() {
        // Zero is the unlimited setting, the same convention the transfer caps
        // use — not "admit nothing", which would stop the app dead.
        let all = [at(1, 0), at(2, 1), at(3, 2)];
        assert_eq!(ids(&all, 0), vec![1, 2, 3]);
    }

    #[test]
    fn the_limit_is_the_number_that_download_at_once() {
        let all = [at(1, 0), at(2, 1), at(3, 2), at(4, 3)];
        assert_eq!(ids(&all, 2), vec![1, 2]);
        assert_eq!(ids(&all, 1), vec![1]);
    }

    #[test]
    fn position_decides_the_turn_rather_than_the_id() {
        // Which is what makes "move to the front" possible at all: the ids are
        // whatever librqbit handed out, and the queue is the user's order.
        let all = [at(1, 5), at(2, 0), at(3, 9)];
        assert_eq!(ids(&all, 2), vec![2, 1]);
    }

    #[test]
    fn moving_to_the_front_is_a_negative_position() {
        // No renumbering of everything else, which is what makes the jump one
        // assignment instead of a pass over the whole list.
        let all = [at(1, 0), at(2, 1), at(3, -1)];
        assert_eq!(ids(&all, 1), vec![3]);
    }

    #[test]
    fn a_paused_torrent_is_not_in_the_queue_at_all() {
        // It does not run, and it does not hold a place: the queue works from
        // what the user wants, not from what happens to be stopped.
        let mut all = [at(1, 0), at(2, 1), at(3, 2)];
        all[0].wanted = false;
        assert_eq!(ids(&all, 2), vec![2, 3]);
    }

    #[test]
    fn seeding_neither_waits_nor_takes_a_place() {
        // A finished torrent costs no download bandwidth, which is the thing
        // being rationed. Stopping it to make room would be taking from the
        // swarm to give to oneself.
        let mut all = [at(1, 0), at(2, 1), at(3, 2)];
        all[0].complete = true;
        assert_eq!(ids(&all, 1), vec![1, 2], "the seed runs, and 2 still gets the one slot");
    }

    #[test]
    fn seeding_runs_even_when_the_queue_is_full() {
        let mut all = [at(1, 0), at(2, 1), at(3, 2)];
        all[2].complete = true;
        assert_eq!(ids(&all, 1), vec![1, 3]);
    }

    #[test]
    fn a_paused_seed_stays_paused() {
        // Complete is not a licence to ignore the user.
        let mut all = [at(1, 0)];
        all[0].complete = true;
        all[0].wanted = false;
        assert!(ids(&all, 0).is_empty());
    }

    #[test]
    fn an_empty_session_admits_nothing_without_panicking() {
        assert!(ids(&[], 3).is_empty());
        assert!(ids(&[], 0).is_empty());
    }
}

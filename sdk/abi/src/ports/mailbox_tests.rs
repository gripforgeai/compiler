//! The mailboxes' criteria (docs/decisions/2026-11-19-canaux-directs.md): in place, deterministic order across
//! boxes, no order ever dropped, a box not yet open, nothing to do at rest, a box not in the directory.

use super::*;

fn take_all(boxes: &[&MailboxHead], tick: u32) -> Vec<(u32, u32, u32, Vec<u64>)> {
    let mut out = Vec::new();
    let mut scratch = Vec::new();
    // SAFETY: the tests are the one reader.
    unsafe {
        take_tick(boxes, tick, &mut scratch, |m| {
            out.push((m.from, m.seq, m.code, m.words.to_vec()))
        })
    };
    out
}

#[test]
fn a_message_is_read_where_it_was_written() {
    let mut dir = Directory::default();
    let at = dir.open(1, "orders", kind::ORDERS, 64);
    let b = unsafe { MailboxHead::at(at) };
    assert!(unsafe { b.write(7, 3, 0, 0x1001, &[1, 2, 3]) });
    let ring = at + std::mem::size_of::<MailboxHead>();
    let mut seen = 0usize;
    unsafe {
        b.peek(|_, m| seen = m.words.as_ptr() as usize);
    }
    // The payload of the first record: after the head and the record's three words. Nothing copied.
    assert_eq!(seen, ring + 3 * 8);
}

#[test]
fn two_boxes_give_one_order_whatever_the_arrival() {
    // A body made by one box, made a vehicle by another, the same tick: the order is the sender's number.
    let run = |reverse: bool| {
        let mut dir = Directory::default();
        let a = unsafe { MailboxHead::at(dir.open(1, "bodies", kind::ORDERS, 64)) };
        let v = unsafe { MailboxHead::at(dir.open(1, "vehicles", kind::ORDERS, 64)) };
        unsafe {
            if reverse {
                v.write(5, 10, 1, 0x1003, &[9]);
                a.write(5, 10, 0, 0x1000, &[1]);
            } else {
                a.write(5, 10, 0, 0x1000, &[1]);
                v.write(5, 10, 1, 0x1003, &[9]);
            }
        }
        take_all(&[v, a], 10)
    };
    assert_eq!(run(false), run(true));
    assert_eq!(run(false)[0].2, 0x1000);
}

#[test]
fn an_order_is_never_dropped_when_its_box_is_full() {
    let mut dir = Directory::default();
    let at = dir.open(1, "orders", kind::ORDERS, 16);
    let mut out = Outbox::new(2);
    for i in 0..10u64 {
        out.send(&dir, 1, "orders", 0, 0x1001, &[i, i, i, i]);
    }
    let b = unsafe { MailboxHead::at(at) };
    assert!(b.full.load(Relaxed) >= 1);
    let mut got = Vec::new();
    while got.len() < 10 {
        got.extend(take_all(&[b], 0).into_iter().map(|m| m.3[0]));
        out.flush(&dir);
    }
    assert_eq!(got, (0..10).collect::<Vec<_>>());
}

#[test]
fn a_state_value_keeps_its_newest_and_counts_the_skipped() {
    let mut dir = Directory::default();
    let at = dir.open(1, "pose", kind::STATE, 16);
    let mut out = Outbox::new(2);
    for i in 0..10u64 {
        out.send(&dir, 1, "pose", 0, 0x1100, &[i, i, i, i]);
    }
    let b = unsafe { MailboxHead::at(at) };
    let mut got = take_all(&[b], 0);
    out.flush(&dir);
    got.extend(take_all(&[b], 0));
    assert_eq!(got.last().unwrap().3[0], 9);
    assert!(got.len() < 10 && b.late.load(Relaxed) > 0);
}

#[test]
fn a_message_sent_before_its_box_waits_then_goes_in_order() {
    let mut dir = Directory::default();
    let mut out = Outbox::new(2);
    for i in 0..5u64 {
        out.send(&dir, 1, "orders", 0, 0x1001, &[i]);
    }
    assert_eq!(out.waiting(), 5);
    let b = unsafe { MailboxHead::at(dir.open(1, "orders", kind::ORDERS, 64)) };
    out.flush(&dir);
    let got: Vec<u64> = take_all(&[b], 0).into_iter().map(|m| m.3[0]).collect();
    assert_eq!(got, vec![0, 1, 2, 3, 4]);
}

#[test]
fn a_box_not_in_the_directory_gets_nothing() {
    // The test that fails when the box is not opened: the capability 0x1234 never arrives.
    let dir = Directory::default();
    let mut out = Outbox::new(2);
    out.send(&dir, 1, "orders", 0, 0x1234, &[1]);
    assert!(dir.resolve(1, "orders").is_none());
    assert_eq!(out.waiting(), 1);
}

#[test]
fn a_later_tick_waits_for_its_step_and_a_gone_one_is_late() {
    let mut dir = Directory::default();
    let b = unsafe { MailboxHead::at(dir.open(1, "orders", kind::ORDERS, 64)) };
    unsafe {
        b.write(1, 4, 0, 0x1001, &[]);
        b.write(1, 6, 1, 0x1001, &[]);
    }
    assert_eq!(take_all(&[b], 5).len(), 1);
    assert_eq!(b.late.load(Relaxed), 1);
    assert_eq!(take_all(&[b], 5).len(), 0);
    assert_eq!(take_all(&[b], 6).len(), 1);
}

#[test]
fn a_box_at_rest_costs_nothing() {
    let mut dir = Directory::default();
    let b = unsafe { MailboxHead::at(dir.open(1, "orders", kind::ORDERS, 64)) };
    assert_eq!(take_all(&[b], 1).len(), 0);
    assert_eq!(b.head.load(Relaxed), 0);
}

#[test]
fn the_ring_wraps_without_losing_a_record() {
    let mut dir = Directory::default();
    let b = unsafe { MailboxHead::at(dir.open(1, "orders", kind::ORDERS, 16)) };
    for i in 0..40u64 {
        assert!(unsafe { b.write(1, 0, i as u32, 0x1001, &[i, i]) });
        assert_eq!(take_all(&[b], 0)[0].3, vec![i, i]);
    }
}

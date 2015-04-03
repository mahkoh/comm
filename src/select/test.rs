use std::thread::{self, sleep_ms};
use std::sync::{Arc};
use std::sync::atomic::{AtomicUsize};
use std::sync::atomic::Ordering::{SeqCst};

use spsc::unbounded::{new};
use super::{Select, Selectable};

fn ms_sleep(ms: i64) {
    sleep_ms(ms as u32);
}

#[test]
fn no_wait_one() {
    let (send, recv) = new();
    send.send(1u8).unwrap();
    let select = Select::new();
    select.add(&recv);
    assert!(select.wait(&mut [0]).len() == 1);
}

#[test]
fn wait_one() {
    let (send, recv) = new();
    thread::spawn(move || {
        ms_sleep(100);
        send.send(1u8).unwrap();
    });
    let select = Select::new();
    select.add(&recv);
    assert!(select.wait(&mut [0]) == &mut [recv.id()][..]);
}

#[test]
fn ready_list_one() {
    let (send, recv) = new();
    let select = Select::new();
    select.add(&recv);
    send.send(1u8).unwrap();
    assert!(select.wait_timeout(&mut [0], None) == Some(&mut [recv.id()][..]));
}

#[test]
fn no_wait_two() {
    let (send, recv) = new();
    let (send2, recv2) = new();
    send.send(1u8).unwrap();
    send2.send(1u8).unwrap();
    let select = Select::new();
    select.add(&recv);
    select.add(&recv2);
    assert!(select.wait(&mut [0, 0]).len() == 2);
}

#[test]
fn wait_two() {
    let (send, recv) = new();
    let (send2, recv2) = new();
    thread::spawn(move || {
        ms_sleep(100);
        send.send(1u8).unwrap();
    });
    thread::spawn(move || {
        ms_sleep(200);
        send2.send(1u8).unwrap();
    });
    let select = Select::new();
    select.add(&recv);
    select.add(&recv2);
    let mut saw1 = false;
    'outer: loop {
        let mut buf = [0, 0];
        for &mut id in select.wait(&mut buf) {
            if id == recv.id() && recv.recv_sync().is_err() {
                saw1 = true;
            }
            if id == recv2.id() && recv2.recv_sync().is_err() {
                break 'outer;
            }
        }
    }
    assert!(saw1);
}

#[test]
fn select_wrong_thread() {
    // Check that cross thread selecting works.

    let (send1, recv1) = new();
    let (send2, recv2) = new();
    let id1 = recv1.id();
    let id2 = recv2.id();
    let select1 = Arc::new(Select::new());
    let select2 = select1.clone();
    let thread = thread::scoped(move || {
        select2.add(&recv2);
        send2.send(1u8).unwrap();
        ms_sleep(100);
        // clear the second channel so that wait below will remove it from the ready list
        recv2.recv_sync().unwrap();
        assert_eq!(select2.wait(&mut [0, 0]), &mut [id1][..]);
    });
    select1.add(&recv1);
    assert_eq!(select1.wait(&mut [0, 0]), &mut [id2][..]);
    send1.send(2u8).unwrap();
    // make sure that we wait for the other thread before dropping anything else
    drop(thread);
}

#[test]
fn select_chance() {
    // Check that only one selecting thread wakes up.

    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = counter1.clone();
    let counter3 = counter1.clone();
    let (send, recv) = new();
    let select1 = Arc::new(Select::new());
    let select2 = select1.clone();
    select1.add(&recv);
    thread::spawn(move || {
        select1.wait(&mut []);
        counter2.fetch_add(1, SeqCst);
    });
    thread::spawn(move || {
        select2.wait(&mut []);
        counter3.fetch_add(1, SeqCst);
    });
    ms_sleep(100);
    send.send(1u8).unwrap();
    ms_sleep(100);
    assert_eq!(counter1.swap(0, SeqCst), 1);
}

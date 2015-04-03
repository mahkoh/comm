use std::sync::{Arc};
use std::thread::{self, sleep_ms};
use std::sync::atomic::{AtomicUsize};
use std::sync::atomic::Ordering::{SeqCst};

use select::{Select, Selectable};
use {Error};

fn ms_sleep(ms: i64) {
    sleep_ms(ms as u32);
}

#[test]
fn send_recv() {
    let channel = super::Channel::new(2);
    channel.send_sync(1u8).unwrap();
    assert_eq!(channel.recv_async().unwrap(), 1u8);
}

#[test]
fn recv_async() {
    let channel = super::Channel::<u8>::new(1);
    assert_eq!(channel.recv_async().unwrap_err(), Error::Empty);
}

#[test]
fn recv_sync() {
    let channel = super::Channel::<u8>::new(2);
    assert_eq!(channel.recv_sync().unwrap_err(), Error::Deadlock);
}

#[test]
fn send_sync() {
    let channel = super::Channel::<u8>::new(1);
    channel.send_sync(1).unwrap();
    assert_eq!(channel.send_sync(1).unwrap_err().1, Error::Deadlock);
}

#[test]
fn send_send() {
    let channel = super::Channel::new(1);
    channel.send_sync(1u8).unwrap();
    assert_eq!(channel.send_async(1u8).unwrap_err().1, Error::Full);
}

#[test]
fn sleep_send_recv() {
    let chan = super::Channel::new(2);
    let chan2 = chan.clone();

    thread::spawn(move || {
        ms_sleep(100);
        chan2.send_sync(1u8).unwrap();
    });

    assert_eq!(chan.recv_sync().unwrap(), 1);
}

#[test]
fn send_sleep_recv() {
    let chan = super::Channel::new(2);
    let chan2 = chan.clone();

    thread::spawn(move || {
        chan2.send_sync(1u8).unwrap();
    });

    ms_sleep(100);
    assert_eq!(chan.recv_sync().unwrap(), 1);
}

#[test]
fn send_sleep_recv_async() {
    let chan = super::Channel::new(2);
    let chan2 = chan.clone();

    thread::spawn(move || {
        chan2.send_sync(1u8).unwrap();
    });

    ms_sleep(100);
    assert_eq!(chan.recv_async().unwrap(), 1);
}

#[test]
fn send_5_recv_5() {
    let chan = super::Channel::new(4);
    chan.send_sync(1u8).unwrap();
    chan.send_sync(2u8).unwrap();
    chan.send_sync(3u8).unwrap();
    chan.send_sync(4u8).unwrap();
    assert_eq!(chan.recv_sync().unwrap(), 1);
    assert_eq!(chan.recv_sync().unwrap(), 2);
    assert_eq!(chan.recv_sync().unwrap(), 3);
    assert_eq!(chan.recv_sync().unwrap(), 4);
    assert_eq!(chan.recv_async().unwrap_err(), Error::Empty);
}

fn multiple_producers_multiple_consumers(buf_size: usize) {
    const NUM_THREADS_PER_END: usize = 2;
    const NUM_PER_THREAD: usize = 1000;
    const RESULT: usize = (NUM_THREADS_PER_END*NUM_PER_THREAD-1)
                                    *(NUM_THREADS_PER_END*NUM_PER_THREAD)/2;

    let chan = super::Channel::<usize>::new(buf_size);
    let sum = Arc::new(AtomicUsize::new(0));
    let mut threads = vec!();
    for _ in 0..NUM_THREADS_PER_END {
        let chan2 = chan.clone();
        let sum2 = sum.clone();
        threads.push(thread::scoped(move || {
            while let Ok(n) = chan2.recv_sync() {
                sum2.fetch_add(n, SeqCst);
            }
        }));
    }
    for i in 0..NUM_THREADS_PER_END {
        let chan2 = chan.clone();
        threads.push(thread::scoped(move || {
            for j in (i*NUM_PER_THREAD..(i+1)*NUM_PER_THREAD) {
                chan2.send_sync(j).unwrap();
            }
        }));
    }
    drop(chan);
    drop(threads);
    assert_eq!(sum.swap(0, SeqCst), RESULT);
}

#[test]
fn multiple_producers_multiple_consumers_1() {
    multiple_producers_multiple_consumers(1);
}

#[test]
fn multiple_producers_multiple_consumers_10() {
    multiple_producers_multiple_consumers(10);
}

#[test]
fn multiple_producers_multiple_consumers_100() {
    multiple_producers_multiple_consumers(100);
}

#[test]
fn multiple_producers_multiple_consumers_1000() {
    multiple_producers_multiple_consumers(1000);
}

#[test]
fn select_no_wait() {
    let chan = super::Channel::new(2);

    chan.send_sync(1u8).unwrap();

    let select = Select::new();
    select.add(&chan);

    let mut buf = [0];
    select.wait(&mut buf);

    assert_eq!(buf[0], chan.id());
}

#[test]
fn select_wait() {
    let chan = super::Channel::new(2);
    let chan2 = chan.clone();

    thread::spawn(move || {
        ms_sleep(100);
        chan2.send_sync(1u8).unwrap();
    });

    let select = Select::new();
    select.add(&chan);

    let mut buf = [0];
    select.wait(&mut buf);

    assert_eq!(buf[0], chan.id());
}

use std::sync::{Arc};
use std::time::duration::{Duration};
use std::thread::{self, sleep};
use std::sync::atomic::{AtomicUsize};
use std::sync::atomic::Ordering::{SeqCst};

use select::{Select, Selectable};
use {Error};

fn ms_sleep(ms: i64) {
    sleep(Duration::milliseconds(ms));
}

#[test]
fn send_recv() {
    let (send, recv) = super::new();
    send.send(1u8).unwrap();
    assert_eq!(recv.recv_async().unwrap(), 1u8);
}

#[test]
fn drop_send_recv() {
    let (send, recv) = super::new::<u8>();
    drop(send);
    assert_eq!(recv.recv_async().unwrap_err(), Error::Disconnected);
}

#[test]
fn drop_recv_send() {
    let (send, recv) = super::new();
    drop(recv);
    assert_eq!(send.send(1u8).unwrap_err(), (1, Error::Disconnected));
}

#[test]
fn recv() {
    let (_send, recv) = super::new::<u8>();
    assert_eq!(recv.recv_async().unwrap_err(), Error::Empty);
}

#[test]
fn sleep_send_recv() {
    let (send, recv) = super::new();

    thread::spawn(move || {
        ms_sleep(100);
        send.send(1u8).unwrap();
    });

    assert_eq!(recv.recv_sync().unwrap(), 1);
}

#[test]
fn send_sleep_recv() {
    let (send, recv) = super::new();

    thread::spawn(move || {
        send.send(1u8).unwrap();
    });

    ms_sleep(100);
    assert_eq!(recv.recv_sync().unwrap(), 1);
}

#[test]
fn send_sleep_recv_async() {
    let (send, recv) = super::new();

    thread::spawn(move || {
        send.send(1u8).unwrap();
    });

    ms_sleep(100);
    assert_eq!(recv.recv_async().unwrap(), 1);
}

#[test]
fn send_5_recv_5() {
    let (send, recv) = super::new();
    send.send(1u8).unwrap();
    send.send(2u8).unwrap();
    send.send(3u8).unwrap();
    send.send(4u8).unwrap();
    assert_eq!(recv.recv_sync().unwrap(), 1);
    assert_eq!(recv.recv_sync().unwrap(), 2);
    assert_eq!(recv.recv_sync().unwrap(), 3);
    assert_eq!(recv.recv_sync().unwrap(), 4);
    assert_eq!(recv.recv_async().unwrap_err(), Error::Empty);
}

#[test]
fn multiple_producers() {
    const NUM: usize = 100;
    const RESULT: usize = (NUM*NUM-1)*(NUM*NUM)/2;

    let (send, recv) = super::new();
    let sum = Arc::new(AtomicUsize::new(0));
    let sum2 = sum.clone();
    let mut threads = vec!();
    threads.push(thread::scoped(move || {
        while let Ok(n) = recv.recv_sync() {
            sum2.fetch_add(n, SeqCst);
        }
    }));
    for i in 0..NUM {
        let send2 = send.clone();
        threads.push(thread::scoped(move || {
            for j in (i*NUM..(i+1)*NUM) {
                send2.send(j).unwrap();
            }
        }));
    }
    drop(send);
    drop(threads);
    assert_eq!(sum.swap(0, SeqCst), RESULT);
}

#[test]
fn select_no_wait() {
    let (send, recv) = super::new();

    send.send(1u8).unwrap();

    let select = Select::new();
    select.add(&recv);

    let mut buf = [0];
    select.wait(&mut buf);

    assert_eq!(buf[0], recv.id());
}

#[test]
fn select_wait() {
    let (send, recv) = super::new();

    thread::spawn(move || {
        ms_sleep(100);
        send.send(1u8).unwrap();
    });

    let select = Select::new();
    select.add(&recv);

    let mut buf = [0];
    select.wait(&mut buf);

    assert_eq!(buf[0], recv.id());
}

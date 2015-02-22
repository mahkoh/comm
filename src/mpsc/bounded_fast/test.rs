use std::old_io::timer::{sleep};
use std::time::duration::{Duration};
use std::{thread};

use select::{Select, Selectable};
use {Error};

fn ms_sleep(ms: i64) {
    sleep(Duration::milliseconds(ms));
}

#[test]
fn send_recv() {
    let (send, recv) = unsafe { super::new(2) };
    send.send_async(1u8).unwrap();
    assert_eq!(recv.recv_async().unwrap(), 1u8);
}

#[test]
fn drop_send_recv() {
    let (send, recv) = unsafe { super::new::<u8>(2) };
    drop(send);
    assert_eq!(recv.recv_async().unwrap_err(), Error::Disconnected);
}

#[test]
fn drop_recv_send() {
    let (send, recv) = unsafe { super::new(2) };
    drop(recv);
    assert_eq!(send.send_async(1u8).unwrap_err(), (1, Error::Disconnected));
}

#[test]
fn recv() {
    let (_send, recv) = unsafe { super::new::<u8>(2) };
    assert_eq!(recv.recv_async().unwrap_err(), Error::Empty);
}

#[test]
fn sleep_send_recv() {
    let (send, recv) = unsafe { super::new(2) };

    thread::spawn(move || {
        ms_sleep(100);
        send.send_async(1u8).unwrap();
    });

    assert_eq!(recv.recv_sync().unwrap(), 1);
}

#[test]
fn send_sleep_recv() {
    let (send, recv) = unsafe { super::new(2) };

    thread::spawn(move || {
        send.send_async(1u8).unwrap();
    });

    ms_sleep(100);
    assert_eq!(recv.recv_sync().unwrap(), 1);
}

#[test]
fn send_sleep_recv_async() {
    let (send, recv) = unsafe { super::new(2) };

    thread::spawn(move || {
        send.send_async(1u8).unwrap();
    });

    ms_sleep(100);
    assert_eq!(recv.recv_async().unwrap(), 1);
}

#[test]
fn send_5_recv_5() {
    let (send, recv) = unsafe { super::new(4) };
    send.send_async(1u8).unwrap();
    send.send_async(2u8).unwrap();
    send.send_async(3u8).unwrap();
    send.send_async(4u8).unwrap();
    assert_eq!(send.send_async(5u8).unwrap_err().1, Error::Full);
    assert_eq!(recv.recv_sync().unwrap(), 1);
    assert_eq!(recv.recv_sync().unwrap(), 2);
    assert_eq!(recv.recv_sync().unwrap(), 3);
    assert_eq!(recv.recv_sync().unwrap(), 4);
    assert_eq!(recv.recv_async().unwrap_err(), Error::Empty);
}

fn multiple_producers(buf_size: usize) {
    const NUM: usize = 100;
    const RESULT: usize = (NUM*NUM-1)*(NUM*NUM)/2;

    let (send, recv) = unsafe { super::new(buf_size) };
    let mut sum = 0;
    let mut threads = vec!();
    for i in 0..NUM {
        let send2 = send.clone();
        threads.push(thread::scoped(move || {
            for j in (i * NUM)..((i+1) * NUM) {
                send2.send_sync(j).unwrap();
            }
        }));
    }
    drop(send);
    while let Ok(i) = recv.recv_sync() {
        sum += i;
    }
    drop(threads);
    assert_eq!(sum, RESULT);
}

#[test]
fn multiple_producers_1() {
    multiple_producers(1);
}

#[test]
fn multiple_producers_10() {
    multiple_producers(10);
}

#[test]
fn multiple_producers_100() {
    multiple_producers(100);
}

#[test]
fn multiple_producers_1000() {
    multiple_producers(1000);
}

#[test]
fn select_no_wait() {
    let (send, recv) = unsafe { super::new(2) };

    send.send_async(1u8).unwrap();

    let select = Select::new();
    select.add(&recv);

    let mut buf = [0];
    select.wait(&mut buf);

    assert_eq!(buf[0], recv.id());
}

#[test]
fn select_wait() {
    let (send, recv) = unsafe { super::new(2) };

    thread::spawn(move || {
        ms_sleep(100);
        send.send_async(1u8).unwrap();
    });

    let select = Select::new();
    select.add(&recv);

    let mut buf = [0];
    select.wait(&mut buf);

    assert_eq!(buf[0], recv.id());
}

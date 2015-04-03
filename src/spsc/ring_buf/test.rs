use std::thread::{self, sleep_ms};

use select::{Select, Selectable};
use {Error};

fn ms_sleep(ms: i64) {
    sleep_ms(ms as u32);
}

#[test]
fn send_recv() {
    let (send, recv) = super::new(2);
    send.send(1u8).unwrap();
    assert_eq!(recv.recv_async().unwrap(), 1u8);
}

#[test]
fn drop_send_recv() {
    let (send, recv) = super::new::<u8>(2);
    drop(send);
    assert_eq!(recv.recv_async().unwrap_err(), Error::Disconnected);
}

#[test]
fn drop_recv_send() {
    let (send, recv) = super::new(2);
    drop(recv);
    assert_eq!(send.send(1u8).unwrap_err(), (1, Error::Disconnected));
}

#[test]
fn recv() {
    let (_send, recv) = super::new::<u8>(2);
    assert_eq!(recv.recv_async().unwrap_err(), Error::Empty);
}

#[test]
fn sleep_send_recv() {
    let (send, recv) = super::new(2);

    thread::spawn(move || {
        ms_sleep(100);
        send.send(1u8).unwrap();
    });

    assert_eq!(recv.recv_sync().unwrap(), 1);
}

#[test]
fn send_sleep_recv() {
    let (send, recv) = super::new(2);

    thread::spawn(move || {
        send.send(1u8).unwrap();
    });

    ms_sleep(100);
    assert_eq!(recv.recv_sync().unwrap(), 1);
}

#[test]
fn send_sleep_recv_async() {
    let (send, recv) = super::new(2);

    thread::spawn(move || {
        send.send(1u8).unwrap();
    });

    ms_sleep(100);
    assert_eq!(recv.recv_async().unwrap(), 1);
}

#[test]
fn send_5_recv_5() {
    let (send, recv) = super::new(3);
    send.send(1u8).unwrap();
    send.send(2u8).unwrap();
    send.send(3u8).unwrap();
    send.send(4u8).unwrap();
    send.send(5u8).unwrap();
    assert_eq!(recv.recv_sync().unwrap(), 2);
    assert_eq!(recv.recv_sync().unwrap(), 3);
    assert_eq!(recv.recv_sync().unwrap(), 4);
    assert_eq!(recv.recv_sync().unwrap(), 5);
    assert_eq!(recv.recv_async().unwrap_err(), Error::Empty);
}

#[test]
fn select_no_wait() {
    let (send, recv) = super::new(2);

    send.send(1u8).unwrap();

    let select = Select::new();
    select.add(&recv);

    let mut buf = [0];
    select.wait(&mut buf);

    assert_eq!(buf[0], recv.id());
}

#[test]
fn select_wait() {
    let (send, recv) = super::new(2);

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

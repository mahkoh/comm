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
fn send_send() {
    let (send, _recv) = super::new();
    send.send(1u8).unwrap();
    assert_eq!(send.send(1u8).unwrap_err(), (1, Error::Full));
}

#[test]
fn send_can_recv() {
    let (send, recv) = super::new();
    send.send(1u8).unwrap();
    assert_eq!(recv.can_recv(), true);
}

#[test]
fn can_recv() {
    let (_send, recv) = super::new::<u8>();
    assert_eq!(recv.can_recv(), false);
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

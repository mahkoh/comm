use std::{thread};

use test::{Bencher, black_box};
use std::{sync};

#[bench]
fn sync_small_stdlib(b: &mut Bencher) {
    let (thread_send, thread_recv) = sync::mpsc::channel::<sync::mpsc::SyncSender<_>>();
    thread::spawn(move || {
        while let Ok(bench_send) = thread_recv.recv() {
            for i in 0..128 {
                bench_send.send(i).unwrap();
            }
        }
    });
    b.iter(|| {
        let (bench_send, bench_recv) = sync::mpsc::sync_channel(2);
        thread_send.send(bench_send).unwrap();
        while let Ok(num) = bench_recv.recv() {
            black_box(num);
        }
    });
}

#[bench]
fn sync_small_comm(b: &mut Bencher) {
    let (thread_send, thread_recv) = sync::mpsc::channel::<super::Producer<_>>();
    thread::spawn(move || {
        while let Ok(bench_send) = thread_recv.recv() {
            for i in 0..128 {
                bench_send.send_sync(i).unwrap();
            }
        }
    });
    b.iter(|| {
        let (bench_send, bench_recv) = super::new(2);
        thread_send.send(bench_send).unwrap();
        while let Ok(num) = bench_recv.recv_sync() {
            black_box(num);
        }
    });
}

#[bench]
fn sync_mid_stdlib(b: &mut Bencher) {
    let (thread_send, thread_recv) = sync::mpsc::channel::<sync::mpsc::SyncSender<_>>();
    thread::spawn(move || {
        while let Ok(bench_send) = thread_recv.recv() {
            for i in 0..128 {
                bench_send.send(i).unwrap();
            }
        }
    });
    b.iter(|| {
        let (bench_send, bench_recv) = sync::mpsc::sync_channel(16);
        thread_send.send(bench_send).unwrap();
        while let Ok(num) = bench_recv.recv() {
            black_box(num);
        }
    });
}

#[bench]
fn sync_mid_comm(b: &mut Bencher) {
    let (thread_send, thread_recv) = sync::mpsc::channel::<super::Producer<_>>();
    thread::spawn(move || {
        while let Ok(bench_send) = thread_recv.recv() {
            for i in 0..128 {
                bench_send.send_sync(i).unwrap();
            }
        }
    });
    b.iter(|| {
        let (bench_send, bench_recv) = super::new(16);
        thread_send.send(bench_send).unwrap();
        while let Ok(num) = bench_recv.recv_sync() {
            black_box(num);
        }
    });
}

#[bench]
fn sync_large_stdlib(b: &mut Bencher) {
    let (thread_send, thread_recv) = sync::mpsc::channel::<sync::mpsc::SyncSender<_>>();
    thread::spawn(move || {
        while let Ok(bench_send) = thread_recv.recv() {
            for i in 0..128 {
                bench_send.send(i).unwrap();
            }
        }
    });
    b.iter(|| {
        let (bench_send, bench_recv) = sync::mpsc::sync_channel(129);
        thread_send.send(bench_send).unwrap();
        while let Ok(num) = bench_recv.recv() {
            black_box(num);
        }
    });
}

#[bench]
fn sync_large_comm(b: &mut Bencher) {
    let (thread_send, thread_recv) = sync::mpsc::channel::<super::Producer<_>>();
    thread::spawn(move || {
        while let Ok(bench_send) = thread_recv.recv() {
            for i in 0..128 {
                bench_send.send_sync(i).unwrap();
            }
        }
    });
    b.iter(|| {
        let (bench_send, bench_recv) = super::new(129);
        thread_send.send(bench_send).unwrap();
        while let Ok(num) = bench_recv.recv_sync() {
            black_box(num);
        }
    });
}

#[bench]
fn async_stdlib(b: &mut Bencher) {
    let (thread_send, thread_recv) =
        sync::mpsc::channel::<(sync::mpsc::SyncSender<_>, sync::mpsc::Sender<_>)>();
    thread::spawn(move || {
        while let Ok((bench_send, notify_send)) = thread_recv.recv() {
            for i in 0..128 {
                bench_send.send(i).unwrap();
            }
            notify_send.send(1).unwrap();
        }
    });
    b.iter(|| {
        let (bench_send, bench_recv) = sync::mpsc::sync_channel(129);
        let (notify_send, notify_recv) = sync::mpsc::channel();
        thread_send.send((bench_send, notify_send)).unwrap();
        notify_recv.recv().unwrap();
        while let Ok(num) = bench_recv.try_recv() {
            black_box(num);
        }
    });
}

#[bench]
fn async_comm(b: &mut Bencher) {
    let (thread_send, thread_recv) =
        sync::mpsc::channel::<(super::Producer<_>, sync::mpsc::Sender<_>)>();
    thread::spawn(move || {
        while let Ok((bench_send, notify_send)) = thread_recv.recv() {
            for i in 0..128 {
                bench_send.send_sync(i).unwrap();
            }
            notify_send.send(1).unwrap();
        }
    });
    b.iter(|| {
        let (bench_send, bench_recv) = super::new(129);
        let (notify_send, notify_recv) = sync::mpsc::channel();
        thread_send.send((bench_send, notify_send)).unwrap();
        notify_recv.recv().unwrap();
        while let Ok(num) = bench_recv.recv_async() {
            black_box(num);
        }
    });
}

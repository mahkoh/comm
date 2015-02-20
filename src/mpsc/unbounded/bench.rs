use std::{thread};

use test::{Bencher, black_box};
use std::{sync};

const NUM_SENDERS: usize = 2;

#[bench]
fn sync_stdlib(b: &mut Bencher) {
    let mut threads = vec!();
    for _ in 0..NUM_SENDERS {
        let (thread_send, thread_recv) = sync::mpsc::channel::<sync::mpsc::Sender<_>>();
        threads.push(thread_send);
        thread::spawn(move || {
            while let Ok(bench_send) = thread_recv.recv() {
                for i in 0..128 {
                    bench_send.send(i).unwrap();
                }
            }
        });
    }
    b.iter(|| {
        let (bench_send, bench_recv) = sync::mpsc::channel();
        for thread in &threads {
            thread.send(bench_send.clone()).unwrap();
        }
        drop(bench_send);
        while let Ok(num) = bench_recv.recv() {
            black_box(num);
        }
    });
}

#[bench]
fn sync_comm(b: &mut Bencher) {
    let mut threads = vec!();
    for _ in 0..NUM_SENDERS {
        let (thread_send, thread_recv) = sync::mpsc::channel::<super::Producer<_>>();
        threads.push(thread_send);
        thread::spawn(move || {
            while let Ok(bench_send) = thread_recv.recv() {
                for i in 0..128 {
                    bench_send.send(i).unwrap();
                }
            }
        });
    }
    b.iter(|| {
        let (bench_send, bench_recv) = super::new();
        for thread in &threads {
            thread.send(bench_send.clone()).unwrap();
        }
        drop(bench_send);
        while let Ok(num) = bench_recv.recv_sync() {
            black_box(num);
        }
    });
}

#[bench]
fn async_stdlib(b: &mut Bencher) {
    let mut threads = vec!();
    for _ in 0..NUM_SENDERS {
        let (thread_send, thread_recv) =
            sync::mpsc::channel::<(sync::mpsc::Sender<_>, sync::mpsc::Sender<_>)>();
        threads.push(thread_send);
        thread::spawn(move || {
            while let Ok((bench_send, notify_send)) = thread_recv.recv() {
                for i in 0..128 {
                    bench_send.send(i).unwrap();
                }
                notify_send.send(1).unwrap();
            }
        });
    }
    b.iter(|| {
        let (notify_send, notify_recv) = sync::mpsc::channel();
        let (bench_send, bench_recv) = sync::mpsc::channel();
        for thread in &threads {
            thread.send((bench_send.clone(), notify_send.clone())).unwrap();
        }
        drop(bench_send);
        for _ in 0..NUM_SENDERS {
            notify_recv.recv().unwrap();
        }
        while let Ok(num) = bench_recv.try_recv() {
            black_box(num);
        }
    });
}

#[bench]
fn async_comm(b: &mut Bencher) {
    let mut threads = vec!();
    for _ in 0..NUM_SENDERS {
        let (thread_send, thread_recv) =
            sync::mpsc::channel::<(super::Producer<_>, sync::mpsc::Sender<_>)>();
        threads.push(thread_send);
        thread::spawn(move || {
            while let Ok((bench_send, notify_send)) = thread_recv.recv() {
                for i in 0..128 {
                    bench_send.send(i).unwrap();
                }
                notify_send.send(1).unwrap();
            }
        });
    }
    b.iter(|| {
        let (notify_send, notify_recv) = sync::mpsc::channel();
        let (bench_send, bench_recv) = super::new();
        for thread in &threads {
            thread.send((bench_send.clone(), notify_send.clone())).unwrap();
        }
        drop(bench_send);
        for _ in 0..NUM_SENDERS {
            notify_recv.recv().unwrap();
        }
        while let Ok(num) = bench_recv.recv_async() {
            black_box(num);
        }
    });
}

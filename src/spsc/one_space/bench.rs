use std::{thread};
use std::{sync};
use test::{Bencher};

#[bench]
fn stdlib(b: &mut Bencher) {
    let (thread_send, thread_recv) = sync::mpsc::channel::<sync::mpsc::Sender<_>>();
    thread::spawn(move || {
        while let Ok(c) = thread_recv.recv() {
            c.send(1).unwrap();
        }
    });
    b.iter(|| {
        let (bench_send, bench_recv) = sync::mpsc::channel();
        thread_send.send(bench_send).unwrap();
        bench_recv.recv()
    });
}

#[bench]
fn comm(b: &mut Bencher) {
    let (thread_send, thread_recv) = sync::mpsc::channel::<super::Producer<_>>();
    thread::spawn(move || {
        while let Ok(c) = thread_recv.recv() {
            c.send(1).unwrap();
        }
    });
    b.iter(|| {
        let (bench_send, bench_recv) = super::new();
        thread_send.send(bench_send).unwrap();
        bench_recv.recv_sync()
    });
}

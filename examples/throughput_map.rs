use rand::seq::SliceRandom;
use rand::thread_rng;
use rand::Rng;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, process, thread};
use vroom::memory::*;
use vroom::vfio::Vfio;
use vroom::Allocating;
use vroom::{NvmeDevice, QUEUE_LENGTH};
pub fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args();
    args.next();

    let pci_addr = match args.next() {
        Some(arg) => arg,
        None => {
            eprintln!("Usage: cargo run --example init <pci bus id>");
            process::exit(1);
        }
    };

    #[allow(clippy::manual_map)]
    let duration = match args.next() {
        Some(secs) => Some(Duration::from_secs(secs.parse().expect(
            "Usage: cargo run --example init <pci bus id> <duration in seconds>",
        ))),
        None => None,
    };

    let duration = duration.unwrap();

    const PAGE_SIZE: usize = PAGESIZE_4KIB;

    Vfio::set_pagesize(PAGE_SIZE);

    let mut nvme = vroom::init(&pci_addr)?;

    let random = true;
    let write = false;

    let nvme = test_throughput_random(nvme, 128, 16, duration, random, write, PAGE_SIZE)?;

    Ok(())
}

fn test_throughput_random(
    nvme: NvmeDevice,
    queue_depth: usize,
    thread_count: u64,
    duration: Duration,
    random: bool,
    write: bool,
    page_size: usize,
) -> Result<NvmeDevice, Box<dyn Error>> {
    println!();
    println!("---------------------------------------------------------------");
    println!();
    println!(
        "Now testing QD{queue_depth} {} with {thread_count} threads.",
        if write { "write" } else { "read" }
    );

    let nvme = qd_n_multithread(
        nvme,
        queue_depth,
        thread_count,
        duration,
        random,
        write,
        page_size,
    )?;

    println!(
        "Tested QD{queue_depth} {} with {thread_count} threads.",
        if write { "write" } else { "read" }
    );

    Ok(nvme)
}

fn qd_n_multithread(
    nvme: NvmeDevice,
    queue_depth: usize,
    thread_count: u64,
    duration: Duration,
    random: bool,
    write: bool,
    page_size: usize,
) -> Result<NvmeDevice, Box<dyn Error>> {
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks;

    let nvme = Arc::new(Mutex::new(nvme));
    let mut threads = Vec::new();

    for _ in 0..thread_count {
        let nvme = Arc::clone(&nvme);
        let range = (0, ns_blocks);

        let handle = thread::spawn(move || -> (u64, f64) {
            let mut rng = rand::thread_rng();
            let bytes = 512 * blocks as usize; // 4kib
            let mut total = std::time::Duration::ZERO;
            let mut buffers = vec![];
            if page_size == PAGESIZE_2MIB {
                let buffer = nvme
                    .lock()
                    .unwrap()
                    .allocate::<u8>(PAGESIZE_2MIB * 4)
                    .unwrap();
                for b in 0..512 * 4 {
                    let slice = buffer.slice(PAGESIZE_4KIB * b..PAGESIZE_4KIB * (b + 1));
                    buffers.push(slice);
                }
            } else {
                for _b in 0..512 {
                    let buffer = nvme.lock().unwrap().allocate::<u8>(PAGESIZE_4KIB).unwrap();
                    buffers.push(buffer);
                }
            }
            buffers.shuffle(&mut rng);
            let mut qpair = nvme
                .lock()
                .unwrap()
                .create_io_queue_pair(QUEUE_LENGTH)
                .unwrap();

            let rand_block = &(0..PAGESIZE_4KIB)
                .map(|_| rand::random::<u8>())
                .collect::<Vec<_>>()[..];
            for b in &mut buffers {
                b[0..PAGESIZE_4KIB].copy_from_slice(rand_block);
            }

            let mut outstanding_ops = 0;
            let mut total_io_ops = 0;
            while total < duration {
                let lba = rng.gen_range(range.0..range.1);
                let before = Instant::now();
                while qpair.quick_poll().is_some() {
                    outstanding_ops -= 1;
                    total_io_ops += 1;
                }
                if outstanding_ops == queue_depth {
                    qpair.complete_io(1);
                    outstanding_ops -= 1;
                    total_io_ops += 1;
                }
                qpair.submit_io(buffers.choose(&mut rng).unwrap(), lba * blocks, write);
                total += before.elapsed();
                outstanding_ops += 1;
            }

            if outstanding_ops != 0 {
                let before = Instant::now();
                qpair.complete_io(outstanding_ops);
                total += before.elapsed();
            }
            total_io_ops += outstanding_ops as u64;
            assert!(qpair.sub_queue.is_empty());
            nvme.lock().unwrap().delete_io_queue_pair(&qpair).unwrap();

            (total_io_ops, total_io_ops as f64 / total.as_secs_f64())
        });
        threads.push(handle);
    }

    let total = threads.into_iter().fold((0, 0.), |acc, thread| {
        let res = thread
            .join()
            .expect("The thread creation or execution failed!");
        (acc.0 + res.0, acc.1 + res.1)
    });

    println!(
        "n: {}, total {} iops: {:?}",
        total.0,
        if write { "write" } else { "read" },
        total.1
    );
    match Arc::try_unwrap(nvme) {
        Ok(mutex) => match mutex.into_inner() {
            Ok(t) => Ok(t),
            Err(e) => Err(e.into()),
        },
        Err(_) => Err("Arc::try_unwrap failed, not the last reference.".into()),
    }
}

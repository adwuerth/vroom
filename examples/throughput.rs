use rand::{thread_rng, Rng};
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, process, thread};
use vroom::memory::*;

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

    let mut nvme = vroom::init(&pci_addr)?;

    let random = true;

    // fill_ns(&mut nvme);

    let nvme = test_throughput_random(nvme, 32, 4, duration, random, true)?;

    Ok(())
}

fn test_throughput_random(
    nvme: NvmeDevice,
    queue_depth: usize,
    thread_count: u64,
    duration: Duration,
    random: bool,
    write: bool,
) -> Result<NvmeDevice, Box<dyn Error>> {
    println!();
    println!("---------------------------------------------------------------");
    println!();
    println!(
        "Now testing QD{queue_depth} {} with {thread_count} threads.",
        if write { "write" } else { "read" }
    );

    let nvme = if queue_depth == 1 && thread_count == 1 {
        qd_1_singlethread(nvme, write, random, duration)?
    } else {
        qd_n_multithread(nvme, queue_depth, thread_count, duration, random, write)?
    };

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
            let bytes = 512 * blocks as usize;
            let mut total = std::time::Duration::ZERO;
            let mut buffer: Dma<u8> =
                Dma::allocate_nvme(vroom::PAGESIZE_4KIB, &nvme.lock().unwrap()).unwrap();

            let mut qpair = nvme
                .lock()
                .unwrap()
                .create_io_queue_pair(QUEUE_LENGTH)
                .unwrap();

            let bytes_mult = queue_depth;
            let rand_block = &(0..(bytes_mult * bytes))
                .map(|_| rand::random::<u8>())
                .collect::<Vec<_>>()[..];
            buffer[0..bytes_mult * bytes].copy_from_slice(rand_block);

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
                qpair.submit_io(
                    &buffer.slice((outstanding_ops * bytes)..(outstanding_ops + 1) * bytes),
                    lba * blocks,
                    write,
                );
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
            nvme.lock().unwrap().delete_io_queue_pair(qpair).unwrap();

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

fn qd_1_singlethread(
    mut nvme: NvmeDevice,
    write: bool,
    random: bool,
    duration: Duration,
) -> Result<NvmeDevice, Box<dyn Error>> {
    let mut buffer: Dma<u8> = Dma::allocate_nvme(HUGE_PAGE_SIZE, &nvme)?;

    let blocks = 8;
    let bytes = 512 * blocks;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1; // - blocks - 1;

    let mut rng = thread_rng();

    let rand_block = &(0..bytes).map(|_| rand::random::<u8>()).collect::<Vec<_>>()[..];
    buffer[..rand_block.len()].copy_from_slice(rand_block);

    let mut total = Duration::ZERO;

    let mut ios = 0;
    let lba = 0;
    while total < duration {
        let lba = if random {
            rng.gen_range(0..ns_blocks)
        } else {
            (lba + 1) % ns_blocks
        };

        let before = Instant::now();
        if write {
            nvme.write(&buffer.slice(0..bytes as usize), lba * blocks)?;
        } else {
            nvme.read(&buffer.slice(0..bytes as usize), lba * blocks)?;
        }
        let elapsed = before.elapsed();
        total += elapsed;
        ios += 1;
    }
    println!(
        "IOP: {ios}, total {} iops: {:?}",
        if write { "write" } else { "read" },
        ios as f64 / total.as_secs_f64()
    );
    Ok(nvme)
}

fn fill_ns(nvme: &mut NvmeDevice) {
    println!("filling namespace");
    let buffer: Dma<u8> = Dma::allocate_nvme(HUGE_PAGE_SIZE, &nvme).unwrap();
    let max_lba = nvme.namespaces.get(&1).unwrap().blocks - buffer.size as u64 / 512 - 1;
    let blocks = buffer.size as u64 / 512;
    let mut lba = 0;
    while lba < max_lba - 512 {
        nvme.write(&buffer, lba).unwrap();
        lba += blocks;
    }
}

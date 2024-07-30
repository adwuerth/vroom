use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};
use std::error::Error;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, process, thread};
use vroom::memory::*;
use vroom::vfio::Vfio;
use vroom::Mapping;
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

    let page_size = match args.next() {
        Some(arg) => arg,
        None => {
            eprintln!("Usage: cargo run --example init <pci bus id> <page size>");
            process::exit(1);
        }
    };

    let page_size = Pagesize::from_str(page_size.as_str())?;

    let duration = duration.unwrap();

    let mut nvme = vroom::init_with_page_size(&pci_addr, page_size.clone())?;

    let random = true;
    let write = true;

    // fill_ns(&mut nvme);

    let nvme = qd_n_multithread(nvme, 32, 4, duration, random, write);

    Ok(())
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

        let handle = thread::spawn(move || -> (u64, f64) {
            let mut rng = rand::thread_rng();
            let mut total = std::time::Duration::ZERO;

            let mut qpair = nvme
                .lock()
                .unwrap()
                .create_io_queue_pair(QUEUE_LENGTH)
                .unwrap();

            const BUFFER_SIZE: usize = PAGESIZE_2MIB * 128;
            const UNIT: usize = PAGESIZE_4KIB;

            let mut buffer = nvme.lock().unwrap().allocate::<u8>(BUFFER_SIZE).unwrap();

            let mut dmas = vec![];

            let rand_block = &(0..BUFFER_SIZE)
                .map(|_| rand::random::<u8>())
                .collect::<Vec<_>>()[..];
            buffer[0..BUFFER_SIZE].copy_from_slice(rand_block);

            for i in 0..(BUFFER_SIZE / UNIT) - 1 {
                dmas.push(buffer.slice(i * UNIT..(i + 1) * UNIT));
            }

            let mut outstanding_ops = 0;
            let mut total_io_ops = 0;
            let lba = 0;
            while total < duration {
                // let lba = rng.gen_range(range.0..range.1);
                let lba = if random {
                    rng.gen_range(0..ns_blocks)
                } else {
                    (lba + 1) % ns_blocks
                };
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
                let submitted_ops = qpair.submit_io(
                    dmas.choose(&mut rand::thread_rng()).unwrap(),
                    lba * blocks,
                    write,
                );

                total += before.elapsed();
                outstanding_ops += submitted_ops;
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

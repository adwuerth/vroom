use rand::Rng;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, process, thread, vec};
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

    let page_size = match args.next() {
        Some(arg) => Pagesize::from_str(&arg)?,
        None => Pagesize::Page2M,
    };

    let duration = duration.unwrap();

    let mut nvme = vroom::init_with_page_size(&pci_addr, page_size)?;

    let random = true;
    let write = false;

    let (nvme, mut latencies) = qd_1_singlethread_latency(nvme, write, random, duration)?;

    write_nanos_to_file(latencies, write)?;

    Ok(())
}

fn write_nanos_to_file(latencies: Vec<u128>, write: bool) -> Result<(), Box<dyn Error>> {
    let mut file = File::create(format!(
        "vroom_qd1_{}_latencies.txt",
        if write { "write" } else { "read" }
    ))?;
    for lat in latencies {
        writeln!(file, "{}", lat)?;
    }
    Ok(())
}

fn qd_n_multithread_latency_nanos(
    nvme: NvmeDevice,
    queue_depth: usize,
    thread_count: u64,
    duration: Duration,
    random: bool,
    write: bool,
) -> Result<(NvmeDevice, Vec<u128>), Box<dyn Error>> {
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks;
    let nvme = Arc::new(Mutex::new(nvme));
    let mut threads = Vec::new();

    let latencies = Arc::new(Mutex::new(Vec::new()));

    for _ in 0..thread_count {
        let nvme = Arc::clone(&nvme);
        let latencies = Arc::clone(&latencies);
        let range = (0, ns_blocks);

        let handle = thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let bytes = 512 * blocks as usize;
            let mut buffer: Dma<u8> =
                Dma::allocate_nvme(PAGESIZE_2MIB, &nvme.lock().unwrap()).unwrap();

            let mut qpair = nvme
                .lock()
                .unwrap()
                .create_io_queue_pair(queue_depth)
                .unwrap();

            let bytes_mult = queue_depth;
            let rand_block = &(0..(bytes_mult * bytes))
                .map(|_| rand::random::<u8>())
                .collect::<Vec<_>>()[..];
            buffer[0..bytes_mult * bytes].copy_from_slice(rand_block);

            let mut outstanding_ops: usize = 0;
            let start_time = Instant::now();

            while start_time.elapsed() < duration {
                let lba = if random {
                    rng.gen_range(range.0..range.1)
                } else {
                    outstanding_ops as u64 % range.1
                };
                let before = Instant::now();

                qpair.submit_io(
                    &buffer.slice((outstanding_ops * bytes)..(outstanding_ops + 1) * bytes),
                    lba * blocks,
                    write,
                );

                outstanding_ops += 1;

                if outstanding_ops == queue_depth {
                    while qpair.quick_poll().is_some() {
                        outstanding_ops -= 1;
                        let latency = before.elapsed().as_nanos();
                        latencies.lock().unwrap().push(latency);
                    }
                }
            }

            while outstanding_ops > 0 {
                if qpair.quick_poll().is_some() {
                    outstanding_ops -= 1;
                    let latency = Instant::now().elapsed().as_nanos();
                    latencies.lock().unwrap().push(latency);
                } else {
                    qpair.complete_io(1).unwrap();
                    outstanding_ops -= 1;
                    let latency = Instant::now().elapsed().as_nanos();
                    latencies.lock().unwrap().push(latency);
                }
            }

            nvme.lock().unwrap().delete_io_queue_pair(&qpair).unwrap();
        });
        threads.push(handle);
    }

    for handle in threads {
        handle
            .join()
            .expect("The thread creation or execution failed!");
    }

    let latencies = Arc::try_unwrap(latencies)
        .expect("Arc::try_unwrap failed, not the last reference.")
        .into_inner()
        .expect("Mutex::into_inner failed.");

    match Arc::try_unwrap(nvme) {
        Ok(mutex) => match mutex.into_inner() {
            Ok(t) => Ok((t, latencies)),
            Err(e) => Err(e.into()),
        },
        Err(_) => Err("Arc::try_unwrap failed, not the last reference.".into()),
    }
}

fn qd_n_multithread(
    nvme: NvmeDevice,
    queue_depth: usize,
    thread_count: u64,
    duration: Duration,
    random: bool,
    write: bool,
) -> Result<(NvmeDevice, Vec<u128>), Box<dyn Error>> {
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks;

    let nvme = Arc::new(Mutex::new(nvme));
    let mut threads = Vec::new();

    let mut latencies: Arc<Mutex<Vec<u128>>> =
        Arc::new(Mutex::new(Vec::<u128>::with_capacity(100_000_000)));

    for _ in 0..thread_count {
        let nvme = Arc::clone(&nvme);
        let latencies = Arc::clone(&latencies);
        let range = (0, ns_blocks);

        let handle = thread::spawn(move || -> (u64, f64) {
            let mut rng = rand::thread_rng();
            let bytes = 512 * blocks as usize; // 4kib
            let mut total = std::time::Duration::ZERO;
            let mut buffer: Dma<u8> =
                Dma::allocate_nvme(vroom::PAGESIZE_4KIB * queue_depth, &nvme.lock().unwrap())
                    .unwrap();

            let mut qpair = nvme
                .lock()
                .unwrap()
                .create_io_queue_pair(QUEUE_LENGTH)
                .unwrap();

            let buffer_size = queue_depth * bytes;

            let rand_block = &(0..buffer_size)
                .map(|_| rand::random::<u8>())
                .collect::<Vec<_>>()[..];
            buffer[0..buffer_size].copy_from_slice(rand_block);

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
                latencies.lock().unwrap().push(before.elapsed().as_nanos());
                total += before.elapsed();
                outstanding_ops += 1;
            }

            if outstanding_ops != 0 {
                let before = Instant::now();
                qpair.complete_io(outstanding_ops);
                latencies.lock().unwrap().push(before.elapsed().as_nanos());
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

    let latencies = Arc::try_unwrap(latencies)
        .expect("Arc::try_unwrap failed, not the last reference.")
        .into_inner()
        .expect("Mutex::into_inner failed.");

    match Arc::try_unwrap(nvme) {
        Ok(mutex) => match mutex.into_inner() {
            Ok(t) => Ok((t, latencies)),
            Err(e) => Err(e.into()),
        },
        Err(_) => Err("Arc::try_unwrap failed, not the last reference.".into()),
    }
}

fn qd_1_singlethread_latency(
    mut nvme: NvmeDevice,
    write: bool,
    random: bool,
    duration: Duration,
) -> Result<(NvmeDevice, Vec<u128>), Box<dyn Error>> {
    let mut buffer: Dma<u8> = Dma::allocate_nvme(PAGESIZE_2MIB, &nvme)?;

    let blocks = 8;
    let bytes = 512 * blocks;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;

    let mut rng = rand::thread_rng();

    let rand_block = &(0..bytes).map(|_| rand::random::<u8>()).collect::<Vec<_>>()[..];
    buffer[..rand_block.len()].copy_from_slice(rand_block);

    let mut total = Duration::ZERO;
    let mut ios = 0;
    let mut lba = 0;
    let mut latencies = Vec::new();

    while total < duration {
        lba = if random {
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
        latencies.push(elapsed.as_nanos());
        total += elapsed;
        ios += 1;
    }

    println!(
        "IOP: {ios}, total {} iops: {:?}",
        if write { "write" } else { "read" },
        ios as f64 / total.as_secs_f64()
    );

    Ok((nvme, latencies))
}

use rand::Rng;
use std::error::Error;
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

    let duration = duration.unwrap();

    let mut nvme = vroom::init(&pci_addr)?;

    let random = true;

    let (nvme, mut latencies) = qd_1_singlethread_latency(nvme, true, true, duration)?;

    to_microseconds(&mut latencies);
    let percentiles = calculate_percentiles(&mut latencies);
    println!("{:?}", percentiles);
    Ok(())
}

fn to_microseconds(latencies: &mut [f64]) {
    for latency in latencies.iter_mut() {
        *latency *= 1_000_000.0;
    }
}

fn calculate_percentiles(latencies: &mut [f64]) -> (f64, f64, Vec<f64>) {
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let len = latencies.len();

    let f = |p: f64| latencies[(len as f64 * p).ceil() as usize - 1];

    let average: f64 = latencies.iter().sum::<f64>() / len as f64;

    let max_latency: f64 = *latencies.last().unwrap();

    (
        average,
        max_latency,
        vec![f(0.90), f(0.99), f(0.999), f(0.9999), f(0.99999)],
    )
}

fn qd_n_multithread_latency(
    nvme: NvmeDevice,
    queue_depth: usize,
    thread_count: u64,
    duration: Duration,
    random: bool,
    write: bool,
) -> Result<(NvmeDevice, Vec<f64>), Box<dyn Error>> {
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
                Dma::allocate_nvme(HUGE_PAGE_SIZE, &nvme.lock().unwrap()).unwrap();

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
            let mut total = std::time::Duration::ZERO;
            let start_time = Instant::now();

            while start_time.elapsed() < duration {
                let lba = rng.gen_range(range.0..range.1);
                let before = Instant::now();

                qpair.submit_io(
                    &buffer.slice((outstanding_ops * bytes)..(outstanding_ops + 1) * bytes),
                    lba * blocks,
                    write,
                );

                while qpair.quick_poll().is_some() {
                    outstanding_ops -= 1;
                    let latency = before.elapsed().as_secs_f64();
                    latencies.lock().unwrap().push(latency);
                }

                if outstanding_ops == queue_depth {
                    qpair.complete_io(1);
                    outstanding_ops -= 1;
                    let latency = before.elapsed().as_secs_f64();
                    latencies.lock().unwrap().push(latency);
                }

                outstanding_ops += 1;
                if outstanding_ops > queue_depth {
                    outstanding_ops = queue_depth;
                }
            }

            if outstanding_ops != 0 {
                let before = Instant::now();
                qpair.complete_io(outstanding_ops);
                let latency = before.elapsed().as_secs_f64();
                latencies.lock().unwrap().push(latency);
            }

            assert!(qpair.sub_queue.is_empty());
            nvme.lock().unwrap().delete_io_queue_pair(qpair).unwrap();
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

fn qd_1_singlethread_latency(
    mut nvme: NvmeDevice,
    write: bool,
    random: bool,
    duration: Duration,
) -> Result<(NvmeDevice, Vec<f64>), Box<dyn Error>> {
    let mut buffer: Dma<u8> = Dma::allocate_nvme(HUGE_PAGE_SIZE, &nvme)?;

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
        latencies.push(elapsed.as_secs_f64());
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

use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};
use std::error::Error;
use std::fs::File;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, process, thread, vec};
use vroom::memory::*;
use vroom::vfio::Vfio;
use vroom::Mapping;
use vroom::{NvmeDevice, QUEUE_LENGTH};

use std::io::Write;
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

    let mut is_mmio = false;
    let page_size = match args.next() {
        Some(arg) => {
            if &arg.to_lowercase() == "mmio1g" {
                is_mmio = true;
                Pagesize::Page1G
            } else if &arg.to_lowercase() == "mmio2m" {
                is_mmio = true;
                Pagesize::Page2M
            } else {
                Pagesize::from_str(&arg)?
            }
        }
        None => {
            eprintln!("Usage: cargo run --example init <pci bus id> <page size>");
            process::exit(1);
        }
    };

    let duration = duration.unwrap();

    let mut nvme = vroom::init_with_page_size(&pci_addr, page_size.clone())?;

    let random = true;
    let write = true;

    let mut data = vec![];

    let mut pages: usize = 1;
    while pages <= 64 {
        let (median, iops) = {
            let (nvme_result, median_result, iops_result) =
                test_throughput_random(nvme, 1, pages as u64, duration, random, write, 512 * 512)?;
            nvme = nvme_result;
            (median_result, iops_result)
        };

        println!("formatting ns 1");
        nvme.format_namespace(Some(1));
        data.push((pages, median, iops));
        pages *= 2;
    }

    {
        let (nvme_result, _, _) = test_throughput_random(nvme, 1, 1, duration, random, write, 512)?;
        nvme = nvme_result;
    }

    println!("formatting ns 1");
    nvme.format_namespace(Some(1));

    let mut file = File::create(format!(
        "qd1lats_{}_{}.csv",
        if is_mmio { "mmio" } else { "vfio" },
        page_size
    ))?;
    writeln!(file, "threads,latency,iops")?;
    for entry in data {
        writeln!(file, "{},{},{}", entry.0, entry.1, entry.2)?;
    }

    Ok(())
}

fn test_throughput_random(
    nvme: NvmeDevice,
    queue_depth: usize,
    thread_count: u64,
    duration: Duration,
    random: bool,
    write: bool,
    ps4k_alloc: usize,
) -> Result<(NvmeDevice, u128, f64), Box<dyn Error>> {
    println!();
    println!("---------------------------------------------------------------");
    println!();
    println!(
        "Now testing QD{queue_depth} {} with {thread_count} threads.",
        if write { "write" } else { "read" }
    );

    let res = qd_1_multithread(
        nvme,
        queue_depth,
        thread_count,
        duration,
        random,
        write,
        ps4k_alloc,
    )?;

    println!(
        "Tested QD{queue_depth} {} with {thread_count} threads.",
        if write { "write" } else { "read" }
    );

    Ok(res)
}

fn qd_1_multithread(
    nvme: NvmeDevice,
    queue_depth: usize,
    thread_count: u64,
    duration: Duration,
    random: bool,
    write: bool,
    ps4k_alloc: usize,
) -> Result<(NvmeDevice, u128, f64), Box<dyn Error>> {
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks;

    let nvme = Arc::new(Mutex::new(nvme));
    let mut threads = Vec::new();

    for thread_num in 0..thread_count {
        let nvme = Arc::clone(&nvme);

        let handle = thread::spawn(move || -> (u64, f64, Vec<u128>) {
            let mut rng = rand::thread_rng();
            let bytes = 512 * blocks as usize; // 4kib
            let mut total = std::time::Duration::ZERO;

            let buffer_size = PAGESIZE_4KIB * ps4k_alloc;

            let mut buffer = nvme.lock().unwrap().allocate::<u8>(buffer_size).unwrap();

            let mut qpair = nvme
                .lock()
                .unwrap()
                .create_io_queue_pair(QUEUE_LENGTH)
                .unwrap();

            let rand_block = &(0..buffer_size)
                .map(|_| rand::random::<u8>())
                .collect::<Vec<_>>()[..];
            buffer[0..buffer_size].copy_from_slice(rand_block);

            let mut buffer_slices = vec![];
            for i in 0..ps4k_alloc {
                buffer_slices.push(buffer.slice(i * PAGESIZE_4KIB..(i + 1) * PAGESIZE_4KIB));
            }
            // buffer_slices.shuffle(&mut thread_rng());
            let mut buffer_slices_it = buffer_slices.iter().cycle();

            let mut outstanding_ops = 0;
            let mut total_io_ops = 0;
            let lba = thread_num;

            let mut latencies = vec![];

            while total < duration {
                // let lba = rng.gen_range(range.0..range.1);
                let lba = if random {
                    rng.gen_range(0..ns_blocks)
                } else {
                    (lba + thread_count) % ns_blocks
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
                qpair.submit_io(buffer_slices_it.next().unwrap(), lba * blocks, write);

                let elapsed = before.elapsed();
                total += elapsed;
                outstanding_ops += 1;
                latencies.push(elapsed.as_nanos());
            }
            if outstanding_ops != 0 {
                let before = Instant::now();
                qpair.complete_io(outstanding_ops);
                total += before.elapsed();
            }
            total_io_ops += outstanding_ops as u64;
            assert!(qpair.sub_queue.is_empty());
            nvme.lock().unwrap().delete_io_queue_pair(&qpair).unwrap();
            nvme.lock().unwrap().deallocate(&buffer).unwrap();

            (
                total_io_ops,
                total_io_ops as f64 / total.as_secs_f64(),
                latencies,
            )
        });
        threads.push(handle);
    }

    let total = threads.into_iter().fold(
        (0, 0., vec![]),
        |mut acc: (u64, f64, Vec<u128>), thread: thread::JoinHandle<(u64, f64, Vec<u128>)>| {
            let res = thread
                .join()
                .expect("The thread creation or execution failed!");
            acc.2.extend(res.2);
            (acc.0 + res.0, acc.1 + res.1, acc.2)
        },
    );

    let median = median(total.2.clone()).unwrap();
    let average = average(total.2).unwrap();
    println!("median: {median}");
    println!("average: {average}");

    println!(
        "n: {}, total {} iops: {:?}",
        total.0,
        if write { "write" } else { "read" },
        total.1
    );
    match Arc::try_unwrap(nvme) {
        Ok(mutex) => match mutex.into_inner() {
            Ok(t) => Ok((t, median, total.1)),
            Err(e) => Err(e.into()),
        },
        Err(_) => Err("Arc::try_unwrap failed, not the last reference.".into()),
    }
}
fn median(mut latencies: Vec<u128>) -> Option<u128> {
    let len = latencies.len();
    if len == 0 {
        return None;
    }
    latencies.sort_unstable();
    if len % 2 == 1 {
        Some(latencies[len / 2])
    } else {
        Some((latencies[len / 2 - 1] + latencies[len / 2]) / 2)
    }
}

fn average(latencies: Vec<u128>) -> Option<f64> {
    let len = latencies.len();
    if len == 0 {
        return None;
    }
    let sum: u128 = latencies.iter().sum();
    Some(sum as f64 / len as f64)
}

fn qd_n_multithread(
    nvme: NvmeDevice,
    queue_depth: usize,
    thread_count: u64,
    duration: Duration,
    random: bool,
    write: bool,
    ps4k_alloc: usize,
) -> Result<NvmeDevice, Box<dyn Error>> {
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks;

    let nvme = Arc::new(Mutex::new(nvme));
    let mut threads = Vec::new();

    for _ in 0..thread_count {
        let nvme = Arc::clone(&nvme);

        let handle = thread::spawn(move || -> (u64, f64) {
            let mut rng = rand::thread_rng();
            let bytes = 512 * blocks as usize; // 4kib
            let mut total = std::time::Duration::ZERO;

            let buffer_size = PAGESIZE_4KIB * ps4k_alloc;

            let mut buffer = nvme.lock().unwrap().allocate::<u8>(buffer_size).unwrap();

            let mut qpair = nvme
                .lock()
                .unwrap()
                .create_io_queue_pair(QUEUE_LENGTH)
                .unwrap();

            let rand_block = &(0..buffer_size)
                .map(|_| rand::random::<u8>())
                .collect::<Vec<_>>()[..];
            buffer[0..buffer_size].copy_from_slice(rand_block);

            let mut buffer_slices = vec![];
            for i in 0..ps4k_alloc {
                buffer_slices.push(buffer.slice(i * PAGESIZE_4KIB..(i + 1) * PAGESIZE_4KIB));
            }
            let mut buffer_slices_it = buffer_slices.iter().cycle();

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
                qpair.submit_io(buffer_slices_it.next().unwrap(), lba * blocks, write);
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

fn qd_n_multithread_alloconce(
    nvme: NvmeDevice,
    queue_depth: usize,
    thread_count: u64,
    duration: Duration,
    random: bool,
    write: bool,
    ps4k_alloc: usize,
) -> Result<NvmeDevice, Box<dyn Error>> {
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks;

    let nvme = Arc::new(Mutex::new(nvme));

    // Allocate buffer once and divide it into chunks
    let buffer_size = PAGESIZE_4KIB * ps4k_alloc;
    let mut buffer = {
        let nvme = nvme.lock().unwrap();
        nvme.allocate::<u8>(buffer_size)?
    };
    let rand_block: Vec<u8> = (0..buffer_size).map(|_| rand::random::<u8>()).collect();
    buffer[0..buffer_size].copy_from_slice(&rand_block);

    // Create slices of the buffer
    let mut buffer_slices = vec![];
    for i in 0..ps4k_alloc {
        buffer_slices.push(buffer.slice(i * PAGESIZE_4KIB..(i + 1) * PAGESIZE_4KIB));
    }

    let buffer_slices = Arc::new(buffer_slices);
    let mut threads = Vec::new();

    for _ in 0..thread_count {
        let nvme = Arc::clone(&nvme);
        let buffer_slices = Arc::clone(&buffer_slices);

        let handle = thread::spawn(move || -> (u64, f64) {
            let mut rng = rand::thread_rng();
            let mut total = Duration::ZERO;

            let mut qpair = nvme
                .lock()
                .unwrap()
                .create_io_queue_pair(QUEUE_LENGTH)
                .unwrap();

            let mut buffer_slices_it = buffer_slices.iter().cycle();

            let mut outstanding_ops = 0;
            let mut total_io_ops = 0;
            let mut lba = 0;

            while total < duration {
                lba = if random {
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

                qpair.submit_io(buffer_slices_it.next().unwrap(), lba * blocks, write);
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

    let total = threads.into_iter().fold((0, 0.0), |acc, thread| {
        let res = thread
            .join()
            .expect("The thread creation or execution failed!");
        (acc.0 + res.0, acc.1 + res.1)
    });

    println!(
        "n: {}, total {} iops: {:.2}",
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
    let blocks = 8;
    let bytes = 512 * blocks;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1; // - blocks - 1;

    let mut rng = thread_rng();
    let mut buffer = nvme.allocate(PAGESIZE_4KIB)?;

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

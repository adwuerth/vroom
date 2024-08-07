use rand::{thread_rng, Rng};
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, process, thread};
use vroom::{memory::*, Mapping};

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

    let nvme = test_throughput_random(nvme, 32, 4, duration, random, false)?;

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

    // let nvme = if queue_depth == 1 && thread_count == 1 {
    //     qd_1_singlethread(nvme, write, random, duration)?
    // } else {
    //     qd_n_multithread(nvme, queue_depth, thread_count, duration, random, write)?
    // };

    let (nvme, result_vec) =
        qd_n_multithread(nvme, queue_depth, thread_count, duration, random, write)?;

    println!("{:?}", result_vec);

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
) -> Result<(NvmeDevice, Vec<f64>), Box<dyn Error>> {
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks;
    let nvme = Arc::new(Mutex::new(nvme));
    let mut threads = Vec::new();

    let interval = Duration::from_secs(1);
    let num_intervals = (duration.as_secs() + interval.as_secs() - 1) / interval.as_secs();

    let throughput_data = Arc::new(Mutex::new(vec![0f64; num_intervals as usize]));

    for _ in 0..thread_count {
        let nvme = Arc::clone(&nvme);
        let throughput_data = Arc::clone(&throughput_data);
        let range = (0, ns_blocks);

        let handle = thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let bytes = 512 * blocks as usize;
            let mut total = std::time::Duration::ZERO;

            let mut buffer = nvme.lock().unwrap().allocate(PAGESIZE_2MIB).unwrap();

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
            let mut interval_io_ops = 0;
            let mut last_instant = Instant::now();
            let start_instant = Instant::now();

            while total < duration {
                let lba = rng.gen_range(range.0..range.1);
                let before = Instant::now();

                while qpair.quick_poll().is_some() {
                    outstanding_ops -= 1;
                    total_io_ops += 1;
                    interval_io_ops += 1;
                }

                if outstanding_ops == queue_depth {
                    qpair.complete_io(1);
                    outstanding_ops -= 1;
                    total_io_ops += 1;
                    interval_io_ops += 1;
                }

                qpair.submit_io(
                    &buffer.slice((outstanding_ops * bytes)..(outstanding_ops + 1) * bytes),
                    lba * blocks,
                    write,
                );

                total += before.elapsed();
                outstanding_ops += 1;

                if last_instant.elapsed() >= interval {
                    let elapsed_intervals =
                        (start_instant.elapsed().as_secs() / interval.as_secs()) as usize;
                    let mut data = throughput_data.lock().unwrap();
                    if elapsed_intervals < data.len() {
                        data[elapsed_intervals] += interval_io_ops as f64 / interval.as_secs_f64();
                    }
                    interval_io_ops = 0;
                    last_instant = Instant::now();
                }
            }

            if outstanding_ops != 0 {
                let before = Instant::now();
                qpair.complete_io(outstanding_ops);
                total += before.elapsed();
            }
            total_io_ops += outstanding_ops as u64;

            assert!(qpair.sub_queue.is_empty());
            nvme.lock().unwrap().delete_io_queue_pair(&qpair).unwrap();
        });
        threads.push(handle);
    }

    for handle in threads {
        handle
            .join()
            .expect("The thread creation or execution failed!");
    }

    let throughput_data = Arc::try_unwrap(throughput_data)
        .expect("Arc::try_unwrap failed, not the last reference.")
        .into_inner()
        .expect("Mutex::into_inner failed.");

    match Arc::try_unwrap(nvme) {
        Ok(mutex) => match mutex.into_inner() {
            Ok(t) => Ok((t, throughput_data)),
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
    let mut buffer = nvme.allocate(PAGESIZE_2MIB)?;

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

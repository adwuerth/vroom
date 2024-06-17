use csv::Writer;
use rand::Rng;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, process, thread, vec};
use vroom::memory::*;
use vroom::vfio::Vfio;

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
    let write = true;

    let (nvme, mut latencies) = qd_1_singlethread_latency(nvme, write, true, duration)?;

    // to_microseconds(&mut latencies);

    write_nanos_to_file(latencies, write)?;

    // let latencies = round_to_nearest_100(latencies);

    // let latencies = nanos_vec_to_microseconds(latencies);

    // write_latencies_to_csv_dup_f64(latencies, &csv_name(&pci_addr, write))?;

    // to_microseconds(&mut latencies);
    // let percentiles = calculate_percentiles(&mut latencies);
    // println!("{:?}", percentiles);
    Ok(())
}

fn csv_name(pci_addr: &str, write: bool) -> String {
    format!(
        "vroom-{}-{}-cdf.csv",
        if Vfio::is_enabled(pci_addr) {
            "vfio"
        } else {
            "mmio"
        },
        if write { "write" } else { "read" }
    )
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

fn seconds_vec_to_microseconds(latencies: &mut [f64]) {
    for latency in latencies.iter_mut() {
        *latency *= 1_000_000.0;
    }
}

fn nanos_vec_to_microseconds(latencies: Vec<u128>) -> Vec<f64> {
    let mut vec = vec![];
    for latency in latencies.into_iter() {
        vec.push(latency as f64 / 1000.0)
    }
    vec
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

fn round_to_nearest_100(ns_vec: Vec<u128>) -> Vec<u128> {
    ns_vec
        .into_iter()
        .map(|ns| {
            let remainder = ns % 10;
            if remainder >= 5 {
                ns + (10 - remainder)
            } else {
                ns - remainder
            }
        })
        .collect()
}

fn qd_n_multithread_latency_nanos(
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

fn qd_1_singlethread_latency(
    mut nvme: NvmeDevice,
    write: bool,
    random: bool,
    duration: Duration,
) -> Result<(NvmeDevice, Vec<u128>), Box<dyn Error>> {
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

fn write_latencies_to_csv(latencies: Vec<f64>, filename: &str) -> Result<(), Box<dyn Error>> {
    let mut wtr = Writer::from_path(filename)?;

    let mut sorted_latencies = latencies.clone();
    sorted_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let percentiles: Vec<f64> = (1..=sorted_latencies.len())
        .map(|i| i as f64 / sorted_latencies.len() as f64 * 100.0)
        .collect();

    wtr.write_record(["latency", "cdf"])?;

    for (latency, percentile) in sorted_latencies.iter().zip(percentiles.iter()) {
        wtr.write_record(&[latency.to_string(), percentile.to_string()])?;
    }

    wtr.flush()?;
    Ok(())
}

fn write_latencies_to_csv_dup_f64(
    latencies: Vec<f64>,
    filename: &str,
) -> Result<(), Box<dyn Error>> {
    let mut wtr = Writer::from_path(filename)?;

    // Count duplicate latencies
    let mut latency_counts: HashMap<u64, (f64, usize)> = HashMap::new();
    for latency in latencies {
        let key = latency.to_bits(); // Convert f64 to u64 bit representation
        let entry = latency_counts.entry(key).or_insert((latency, 0));
        entry.1 += 1;
    }

    // Create a sorted list of unique latencies
    let mut unique_latencies: Vec<(f64, usize)> =
        latency_counts.into_iter().map(|(_, v)| v).collect();
    unique_latencies.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    // Calculate percentiles for grouped data
    let mut total_count = 0;
    for (_, count) in &unique_latencies {
        total_count += count;
    }

    let mut cumulative_count = 0;
    let mut percentile_data = Vec::new();
    for (latency, count) in unique_latencies {
        cumulative_count += count;
        let percentile = cumulative_count as f64 / total_count as f64;
        percentile_data.push((latency, percentile));
    }

    // Write header
    wtr.write_record(&["latency", "cdf"])?;

    // Write data
    for (latency, percentile) in percentile_data {
        if percentile >= 1.0 || latency > 1000.0 {
            continue;
        }
        wtr.write_record(&[latency.to_string(), percentile.to_string()])?;
    }

    wtr.flush()?;
    Ok(())
}

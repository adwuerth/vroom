use csv::Writer;
use rand::Rng;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, process, thread, vec};
use vroom::vfio::Vfio;
use vroom::{memory::*, Allocating};

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

    let mut nvme = vroom::init(&pci_addr)?;

    let random = true;
    let write = true;

    let mut operations = 10000;

    // for _ in 0..11 {
    //     let res = qd_1_singlethread_latency(nvme, write, random, operations + 1)?;
    //     nvme = res.0;
    //     let latencies = res.1;

    //     write_nanos_to_file_2(latencies, write, operations)?;
    //     operations *= 2;
    // }

    let res = qd_1_singlethread_latency(nvme, write, random, operations)?;
    nvme = res.0;
    let latencies = res.1;
    write_nanos_to_file_2(latencies, write, operations)?;

    Ok(())
}

fn qd_1_singlethread_latency(
    mut nvme: NvmeDevice,
    write: bool,
    random: bool,
    operations: u32,
) -> Result<(NvmeDevice, Vec<u128>), Box<dyn Error>> {
    let blocks = 8;
    let bytes = 512 * blocks; // 4KiB
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;

    let mut rng = rand::thread_rng();

    let rand_block = &(0..bytes).map(|_| rand::random::<u8>()).collect::<Vec<_>>()[..];

    let mut total = Duration::ZERO;
    let mut ios = 0;
    let mut lba = 0;
    let mut latencies = Vec::new();

    let mut buffer: Dma<u8> = nvme.allocate(PAGESIZE_2MIB)?;

    buffer[..rand_block.len()].copy_from_slice(rand_block);

    for i in 0..operations {
        lba = if random {
            rng.gen_range(0..ns_blocks)
        } else {
            (lba + 1) % ns_blocks
        };

        let elapsed_alloc = if true {
            let before_alloc = Instant::now();
            let mut buffer: Dma<u8> = nvme.allocate(PAGESIZE_2MIB)?;
            let elapsed_alloc = before_alloc.elapsed();

            buffer[..rand_block.len()].copy_from_slice(rand_block);
            elapsed_alloc
        } else {
            Duration::ZERO
        };

        let before = Instant::now();
        if write {
            nvme.write(&buffer.slice(0..bytes as usize), lba * blocks)?;
        } else {
            nvme.read(&buffer.slice(0..bytes as usize), lba * blocks)?;
        }
        let elapsed = before.elapsed() + elapsed_alloc;
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

fn write_nanos_to_file(latencies: Vec<u128>, write: bool, size: u32) -> Result<(), Box<dyn Error>> {
    let mut file = File::create(format!(
        "vroom_qd1_{}_latencies_{}.txt",
        if write { "write" } else { "read" },
        size
    ))?;
    let mut ctr: u32 = 0;
    for (ctr, lat) in (0_u32..).zip(latencies.into_iter()) {
        writeln!(file, "{}, {}", ctr, lat)?;
    }
    Ok(())
}

fn write_nanos_to_file_2(
    latencies: Vec<u128>,
    write: bool,
    size: u32,
) -> Result<(), Box<dyn Error>> {
    let mut file = File::create(format!(
        "vroom_qd1_{}_latencies_{}.txt",
        if write { "write" } else { "read" },
        size
    ))?;
    for lat in latencies {
        writeln!(file, "{}", lat)?;
    }
    Ok(())
}

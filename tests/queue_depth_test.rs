mod common;
use common::*;
use rand::Rng;
use std::error::Error;
use std::process;
use std::time::{Duration, Instant};
use vroom::memory::{Dma, DmaSlice};
use vroom::HUGE_PAGE_SIZE;
use vroom::{NvmeDevice, QUEUE_LENGTH};

#[test]
pub fn qd_n_test() {
    let pci_addr = &get_pci_addr();

    let mut nvme = init_nvme(pci_addr);

    let duration: Duration = Duration::from_secs(5);

    let write = true;

    let mut queue_depth = 1;
    for _i in 0..10 {
        println!("queue depth: {}", queue_depth);
        nvme = match qd_n(nvme, queue_depth, duration, write) {
            Ok(nvme) => nvme,
            Err(e) => {
                eprintln!("qd_{} randwrite failed: {}", queue_depth, e);
                process::exit(1);
            }
        };
        queue_depth *= 2;
    }
}

fn qd_n(
    mut nvme: NvmeDevice,
    queue_depth: usize,
    duration: Duration,
    write: bool,
) -> Result<NvmeDevice, Box<dyn Error>> {
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks;

    let range = (0, ns_blocks);

    let mut rng = rand::thread_rng();
    let bytes = 512 * blocks as usize;
    let mut total = std::time::Duration::ZERO;

    let mut buffer: Dma<u8> = allocate_dma_buffer(&nvme, HUGE_PAGE_SIZE);

    let mut qpair = nvme.create_io_queue_pair(QUEUE_LENGTH).unwrap_or_else(|e| {
        eprintln!("Creation of IO Queue Pair failed: {}", e);
        process::exit(1);
    });

    let bytes_mult = queue_depth;
    let rand_block = &(0..(bytes_mult * bytes))
        .map(|_| rand::random::<u8>())
        .collect::<Vec<_>>()[..];
    buffer[0..bytes_mult * bytes].copy_from_slice(rand_block);

    let mut outstanding_ops = 0;
    while total < duration {
        let lba = rng.gen_range(range.0..range.1);
        let before = Instant::now();
        while qpair
            .quick_poll_result()
            .unwrap_or_else(|e| {
                eprintln!("Deletion of io queue pair failed: {}", e);
                process::exit(1);
            })
            .is_some()
        {
            outstanding_ops -= 1;
        }
        if outstanding_ops == queue_depth {
            let io_result = qpair.complete_io(1);
            if io_result.is_none() {
                eprintln!("IO Completion failed!");
                process::exit(1);
            }
            outstanding_ops -= 1;
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
    assert!(qpair.sub_queue.is_empty());
    nvme.delete_io_queue_pair(&qpair).unwrap_or_else(|e| {
        eprintln!("Deletion of io queue pair failed: {}", e);
        process::exit(1);
    });

    Ok(nvme)
}

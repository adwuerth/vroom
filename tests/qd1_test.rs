use rand::{thread_rng, Rng};
use std::error::Error;
use std::process;
use std::time::{Duration, Instant};
use vroom::memory::{Dma, DmaSlice};
use vroom::HUGE_PAGE_SIZE;
use vroom::{NvmeDevice, QUEUE_LENGTH};

mod common;
use common::*;

#[test]
pub fn qd1_test() {
    let pci_addr = &get_pci_addr();

    let nvme = init_nvme(pci_addr);

    let duration: Duration = Duration::from_secs(15);

    let nvme = match qd1(nvme, true, true, duration) {
        Ok(nvme) => nvme,
        Err(e) => {
            eprintln!("qd1 randwrite failed: {}", e);
            process::exit(1);
        }
    };

    let nvme = match qd1(nvme, false, true, duration) {
        Ok(nvme) => nvme,
        Err(e) => {
            eprintln!("qd1 randread failed: {}", e);
            process::exit(1);
        }
    };
}

fn qd1(
    mut nvme: NvmeDevice,
    write: bool,
    random: bool,
    time: Duration,
) -> Result<NvmeDevice, Box<dyn Error>> {
    let mut buffer: Dma<u8> = allocate_dma_buffer(&nvme, HUGE_PAGE_SIZE);

    let blocks = 8;
    let bytes = 512 * blocks;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1; // - blocks - 1;

    let mut rng = thread_rng();

    let rand_block = &(0..bytes).map(|_| rand::random::<u8>()).collect::<Vec<_>>()[..];
    buffer[..rand_block.len()].copy_from_slice(rand_block);

    let mut total = Duration::ZERO;

    let lba = 0;
    while total < time {
        let lba = if random {
            rng.gen_range(0..ns_blocks)
        } else {
            (lba + 1) % ns_blocks
        };

        let before = Instant::now();
        if write {
            nvme_write(&mut nvme, &buffer.slice(0..bytes as usize), lba * blocks);
            nvme_read(
                &mut nvme,
                &mut buffer.slice(0..bytes as usize),
                lba * blocks,
            );
            let read_buf = &buffer[0..rand_block.len()];
            assert_eq!(
                rand_block, read_buf,
                "Data read from NVMe does not match expected data"
            );
        } else {
            nvme_read(
                &mut nvme,
                &mut buffer.slice(0..bytes as usize),
                lba * blocks,
            );
        }

        let elapsed = before.elapsed();
        total += elapsed;
    }

    Ok(nvme)
}

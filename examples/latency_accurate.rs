use rand::seq::SliceRandom;
use rand::thread_rng;
use rand::Rng;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::time::Instant;
use std::{env, process, vec};
use vroom::vfio::Vfio;
use vroom::{memory::*, Allocating};
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

    // CONFIG
    let random = false;
    let write = false;
    const BUFFER_MULT: usize = 512;
    //CURRENTLY ONLY SUPPORTS 4KIB
    const DMA_SIZE: usize = PAGESIZE_4KIB;
    const PAGE_SIZE: usize = PAGESIZE_4KIB;
    const ALWAYS_SAME_DMA: bool = false;
    const SKIP_LATENCIES: usize = 0;
    const THRESHOLD: u128 = 7500 * 1000000;
    const SHUFFLE_HALFWAY: bool = true;
    const SHUFFLE_FIRST: bool = true;

    Vfio::set_pagesize(PAGE_SIZE);
    let mut latencies: Vec<u128> = vec![];

    let blocks = 8;
    // let bytes = 512 * blocks;
    // let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;

    let mut rng = rand::thread_rng();

    let mut lba = 0;

    let vfio = {
        match nvme.allocator {
            vroom::ioallocator::IOAllocator::MmioAllocator(_) => panic!(""),
            vroom::ioallocator::IOAllocator::VfioAllocator(ref vfio) => vfio,
        }
    };

    for _ in 0..BUFFER_MULT {
        let ptr = Vfio::allocate_with_pagesize(PAGESIZE_2MIB);
        let dma = vfio.map_dma::<u8>(ptr, PAGESIZE_2MIB)?;
        lba += 1;
        for i in 0..512 {
            let dma_slice = &dma.slice(i * PAGESIZE_4KIB..(i + 1) * PAGESIZE_4KIB);

            nvme.read(dma_slice, lba)?;
        }
    }

    write_nanos_to_file(
        latencies[SKIP_LATENCIES..].to_vec(),
        write,
        DMA_SIZE,
        PAGE_SIZE,
        BUFFER_MULT,
        false,
        ALWAYS_SAME_DMA,
    )?;
    // write_nanos_to_file(
    //     latencies_in_iotlb[SKIP_LATENCIES..].to_vec(),
    //     write,
    //     DMA_SIZE,
    //     PAGE_SIZE,
    //     BUFFER_MULT,
    //     true,
    //     ALWAYS_SAME_DMA,
    // )?;
    Ok(())
}

fn write_nanos_to_file(
    latencies: Vec<u128>,
    write: bool,
    dma_size: usize,
    page_size: usize,
    buffer_mult: usize,
    second_run: bool,
    same: bool,
) -> Result<(), Box<dyn Error>> {
    const IOMMU: &str = "tn";
    let mut file = File::create(format!(
        "latency_intmap_{}_{}ds_{}ps_{buffer_mult}_{IOMMU}_{}_{}.txt",
        if write { "write" } else { "read" },
        size_to_string(dma_size),
        size_to_string(page_size),
        if second_run { "second" } else { "first" },
        if same { "same" } else { "diff" }
    ))?;
    for lat in latencies {
        writeln!(file, "{}", lat)?;
    }
    Ok(())
}

fn size_to_string(size: usize) -> String {
    let s = if size == PAGESIZE_4KIB {
        "4kib"
    } else if size == PAGESIZE_2MIB {
        "2mib"
    } else {
        "unknown"
    };
    s.to_owned()
}

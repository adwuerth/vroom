use core::panic;
use std::error::Error;

use vroom::ioallocator::IOAllocator;
use vroom::vfio::Vfio;
use vroom::{memory::*, Allocating};

use std::fs::{self};
use std::io::Write;

use std::{env, process};

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
    nvme.set_page_size(Pagesize::Page2M);

    let allocator = &nvme.allocator;

    let mmio = match allocator.as_ref() {
        IOAllocator::VfioAllocator(_) => panic!(""),
        IOAllocator::MmioAllocator(mmio) => mmio,
    };

    let allocate_output = "outputallocate_mmio.txt";
    fs::remove_file(allocate_output).ok();
    let mut allocate_output = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(allocate_output)?;

    const ITERATIONS: u32 = 2 << 7;
    const ALLOC_SIZE: usize = PAGESIZE_2MIB;

    for _ in 0..ITERATIONS {
        let start_time = std::time::Instant::now();
        let _dma = mmio.allocate::<u8>(ALLOC_SIZE)?;
        let elapsed = start_time.elapsed();
        writeln!(allocate_output, "{:?}", elapsed.as_nanos()).unwrap();
    }

    Ok(())
}

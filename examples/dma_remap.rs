use std::error::Error;
use std::time::Instant;

use vroom::ioallocator::IOAllocator;
use vroom::memory::*;
use vroom::vfio::Vfio;

use std::{env, process};
use vroom::Allocating;
use vroom::NvmeDevice;

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

    // let mut nvme = vroom::init(&pci_addr)?;

    println!("NVMe device initialized");

    // let allocator = nvme.allocator;

    // let vfio = match allocator {
    //     IOAllocator::VfioAllocator(vfio) => vfio,
    //     _ => {
    //         eprintln!("Not using VFIO");
    //         process::exit(1);
    //     }
    // };

    let vfio = Vfio::init(&pci_addr)?;

    println!("VFIO initialized");

    vfio.map_resource()?;
    println!("Mapped resource");

    let size = PAGESIZE_2MIB;

    let ptr = vfio.manual_alloc(size)?;

    println!("Allocated manual memory at {:p}", ptr);

    vfio.map_dma::<u8>(ptr, size)?;

    println!("Mapped memory");

    let start_time = Instant::now();

    vfio.unmap_dma(ptr, size)?;
    println!("Unmapped memory");
    vfio.map_dma::<u8>(ptr, size)?;
    println!("Remapped memory");

    let elapsed = start_time.elapsed();

    println!("Time to remap: {:?}", elapsed);

    Ok(())
}

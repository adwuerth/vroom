use std::error::Error;

use vroom::memory::*;

use std::{env, process};
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

    let mut nvme = vroom::init(&pci_addr)?;

    fill_ns(&mut nvme);

    Ok(())
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

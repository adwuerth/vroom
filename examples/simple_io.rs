use std::error::Error;
use std::str;
use std::{env, process};
use vroom::memory::{vfio_enabled, Dma};
use vroom::vfio;
use vroom::HUGE_PAGE_SIZE;
use vroom::vfio::Vfio;

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

    // Logical Block Adress
    let lba = 0;

    println!("vfio enabled? {:?}", vfio_enabled());
    println!("is intel iommu? {:?}", Vfio::is_intel_iommu(&pci_addr));
    println!("gaw: {:?}", Vfio::get_intel_iommu_gaw(&pci_addr));

    // Initialize NVMe Driver
    let mut nvme = vroom::init(&pci_addr)?;

    // Add Test bytes and copy to DMA
    let bytes: &[u8] = "hello world! vroom test bytes".as_bytes();
    let mut buffer: Dma<u8> = Dma::allocate(HUGE_PAGE_SIZE)?;
    buffer[..bytes.len()].copy_from_slice(bytes);

    // Write the bytes to the NVMe memory
    nvme.write(&buffer, lba)?;

    // Empty the buffer
    buffer[..bytes.len()].fill_with(Default::default);

    // Read the written bytes
    nvme.read(&buffer, lba)?;
    let read_buf = &buffer[0..bytes.len()];
    println!("read bytes: {:?}", read_buf);
    println!("read string: {}", str::from_utf8(read_buf).unwrap());
    Ok(())
}

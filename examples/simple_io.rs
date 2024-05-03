use std::error::Error;
use std::str;
use std::{env, process};
use vroom::memory::{vfio_enabled, Dma};
use vroom::vfio;
use vroom::HUGE_PAGE_SIZE;

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

    // println!("now starting simple example");

    // let mut nvme = vroom::init(&pci_addr)?;

    // println!("passed initialization");

    // nvme.identify_controller()?;

    // Logical Block Adress
    let lba = 0;

    // write random bytes to buffer
    // let blocks = 8;
    // let bytes = 512 * blocks;
    // let rand_block = &(0..bytes).map(|_| rand::random::<u8>()).collect::<Vec<_>>()[..];
    // buffer[..rand_block.len()].copy_from_slice(rand_block);

    println!("vfio enabled? {:?}", vfio_enabled());

    println!("is intel iommu? {:?}", vfio::vfio_is_intel_iommu(&pci_addr));
    println!("gaw: {:?}", vfio::vfio_get_intel_iommu_gaw(&pci_addr));

    let vfio_ptr = vfio::vfio_init(&pci_addr)?;
    println!("vfio init done");
    // let mut nvme = vroom::init(&pci_addr)?;
    // println!("vroom init done");
    let bytes: &[u8] = "omg did this work????????".as_bytes();
    let mut buffer: Dma<u8> = Dma::allocate(HUGE_PAGE_SIZE)?;
    println!("allocate done");
    // println!("page alloc done");
    buffer[..bytes.len()].copy_from_slice(bytes);
    println!("copied buffer");

    let mut nvme = vroom::init(&pci_addr)?;
    nvme.write(&buffer, lba)?;

    buffer[..bytes.len()].fill_with(Default::default);
    println!("write done");
    nvme.read(&buffer, lba)?;
    println!("read buffer: {:?}", buffer);

    let read_buf = &buffer[0..bytes.len()];

    println!("read string: {}", str::from_utf8(&read_buf).unwrap());

    println!("read buffer: {:?}", &buffer[0..bytes.len()]);
    // assert_eq!(&buffer[0..rand_block.len()], rand_block);
    Ok(())
}

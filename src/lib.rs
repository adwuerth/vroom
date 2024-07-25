#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::module_name_repetitions
)]
#![cfg_attr(target_arch = "aarch64", feature(stdarch_arm_hints))]
#[allow(unused)]
mod cmd;
pub mod mapping;
#[allow(dead_code)]
pub mod memory;
mod mmio;
#[allow(dead_code)]
mod nvme;
#[allow(dead_code)]
mod pci;
#[allow(dead_code)]
mod queues;
pub mod vfio;

#[allow(dead_code, clippy::identity_op)]
mod vfio_constants;

mod vfio_structs;

use memory::Pagesize;
use memory::DEFAULT_PAGE_SIZE;
pub use memory::PAGESIZE_1GIB;
pub use memory::PAGESIZE_2MIB;
pub use memory::PAGESIZE_4KIB;

pub use mapping::Mapping;
pub use mapping::MemoryMapping;

pub use nvme::{NvmeDevice, NvmeQueuePair};
use pci::{pci_open_resource_ro, read_hex, read_io32};
pub use queues::QUEUE_LENGTH;
use std::error::Error;

/// initialise driver
/// # Arguments
/// * `pci_addr` - pci address of the device
/// # Panics
/// Panics if the device cant be found
/// # Errors
/// Returns an error if the device is not a block device/nvme, or if the device can not be initialised
pub fn init(pci_addr: &str) -> Result<NvmeDevice, Box<dyn Error>> {
    init_with_page_size(pci_addr, DEFAULT_PAGE_SIZE)
}

/// initialise driver
/// # Arguments
/// * `pci_addr` - pci address of the device
/// * `page_size` - page size for VFIO, MMIO only works with 2mib
/// # Panics
/// Panics if the device cant be found
/// # Errors
/// Returns an error if the device is not a block device/nvme, or if the device can not be initialised
pub fn init_with_page_size(
    pci_addr: &str,
    page_size: Pagesize,
) -> Result<NvmeDevice, Box<dyn Error>> {
    let mut vendor_file = pci_open_resource_ro(pci_addr, "vendor").expect("wrong pci address");
    let mut device_file = pci_open_resource_ro(pci_addr, "device").expect("wrong pci address");
    let mut config_file = pci_open_resource_ro(pci_addr, "config").expect("wrong pci address");

    let _vendor_id = read_hex(&mut vendor_file)?;
    let _device_id = read_hex(&mut device_file)?;
    let class_id = read_io32(&mut config_file, 8)? >> 16;

    // 0x01 -> mass storage device class id
    // 0x08 -> nvme subclass
    if class_id != 0x0108 {
        return Err(format!("device {pci_addr} is not a block device").into());
    }

    let allocator = MemoryMapping::init_with_page_size(pci_addr, page_size)?;
    let mut nvme = NvmeDevice::init(pci_addr, Box::new(allocator))?;
    nvme.identify_controller()?;
    let ns = nvme.identify_namespace_list(0);
    for n in ns {
        println!("ns_id: {n}");
        nvme.identify_namespace(n);
    }
    Ok(nvme)
}

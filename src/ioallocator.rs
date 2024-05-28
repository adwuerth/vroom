use crate::memory::Dma;
use crate::uio::Uio;
use crate::vfio::Vfio;
use std::error::Error;

pub trait Allocating {
    /// Allocate Dma<T> with size
    fn allocate<T>(&self, size: usize) -> Result<Dma<T>, Box<dyn Error>>;

    /// Map Resource/Region
    fn map_resource(&self) -> Result<(*mut u8, usize), Box<dyn Error>>;
}

/// IOAllocators UIO and VFIO, is necessary such that trait Allocating can be used as a object
pub enum IOAllocator {
    UioAllocator(Uio),
    VfioAllocator(Vfio),
}

impl IOAllocator {
    /// Returns either UIO or VFIO, depending on if vfio is enabled
    pub fn init(pci_addr: &str) -> Result<IOAllocator, Box<dyn Error>> {
        Ok(if Vfio::is_enabled(pci_addr) {
            Self::VfioAllocator(Vfio::init(pci_addr)?)
        } else {
            if unsafe { libc::getuid() } != 0 {
                println!("not running as root, this will probably fail");
            }
            Self::UioAllocator(Uio::init(pci_addr)?)
        })
    }
}

impl Allocating for IOAllocator {
    fn allocate<T>(&self, size: usize) -> Result<Dma<T>, Box<dyn Error>> {
        match self {
            Self::UioAllocator(uio) => uio.allocate(size),
            Self::VfioAllocator(vfio) => vfio.allocate(size),
        }
    }

    fn map_resource(&self) -> Result<(*mut u8, usize), Box<dyn Error>> {
        match self {
            Self::UioAllocator(uio) => uio.map_resource(),
            Self::VfioAllocator(vfio) => vfio.map_resource(),
        }
    }
}

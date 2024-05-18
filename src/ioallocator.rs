use std::error::Error;
use crate::memory::Dma;
use crate::uio::Uio;
use crate::vfio::Vfio;

pub trait Allocating {
    fn allocate<T>(&self, size: usize) -> Result<Dma<T>, Box<dyn Error>>;

    fn map_resource(&self) -> Result<(*mut u8, usize), Box<dyn Error>>;
}

/// IOAllocators UIO and VFIO, is necessary such that trait Allocating can be used as a object
pub enum IOAllocator {
    UioAllocator(Uio),
    VfioAllocator(Vfio)
}

impl Allocating for IOAllocator {
    fn allocate<T>(&self, size: usize) -> Result<Dma<T>, Box<dyn Error>> {
        match self {
            IOAllocator::UioAllocator(uio) => {uio.allocate(size)}
            IOAllocator::VfioAllocator(vfio) => {vfio.allocate(size)}
        }
    }

    fn map_resource(&self) -> Result<(*mut u8, usize), Box<dyn Error>> {
        match self {
            IOAllocator::UioAllocator(uio) => {uio.map_resource()}
            IOAllocator::VfioAllocator(vfio) => {vfio.map_resource()}
        }
    }
}
use std::error::Error;
use std::ops::{Deref, DerefMut, Index, IndexMut, Range, RangeFull, RangeTo};
use std::slice;

use crate::ioallocator::{Allocating, IOAllocator};
use crate::NvmeDevice;

pub const HUGE_PAGE_BITS: u32 = 21;
pub const HUGE_PAGE_SIZE: usize = 1 << HUGE_PAGE_BITS;

pub const PAGESIZE_4KIB: usize = 1024 * 4;
pub const PAGESIZE_2MIB: usize = 1024 * 1024 * 2;
pub const PAGESIZE_1GIB: usize = 1024 * 1024 * 1024;

#[derive(Debug)]
pub struct Dma<T> {
    pub virt: *mut T,
    pub phys: usize,
    pub size: usize,
}

// should be safe
impl<T> Deref for Dma<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.virt }
    }
}

impl<T> DerefMut for Dma<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.virt }
    }
}

// Trait for types that can be viewed as DMA slices
pub trait DmaSlice {
    type Item;

    fn chunks(&self, bytes: usize) -> DmaChunks<u8>;
    fn slice(&self, range: Range<usize>) -> Self::Item;
}

// mildly overengineered lol
pub struct DmaChunks<'a, T> {
    current_offset: usize,
    chunk_size: usize,
    dma: &'a Dma<T>,
}

impl<'a, T> Iterator for DmaChunks<'a, T> {
    type Item = DmaChunk<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_offset >= self.dma.size {
            None
        } else {
            let chunk_phys_addr = self.dma.phys + self.current_offset * std::mem::size_of::<T>();
            let offset_ptr = unsafe { self.dma.virt.add(self.current_offset) };
            let len = std::cmp::min(
                self.chunk_size,
                (self.dma.size - self.current_offset) / std::mem::size_of::<T>(),
            );

            self.current_offset += len;

            Some(DmaChunk {
                phys_addr: chunk_phys_addr,
                slice: unsafe { std::slice::from_raw_parts_mut(offset_ptr, len) },
            })
        }
    }
}

// Represents a chunk obtained from a Dma<T>, with physical address and slice.
pub struct DmaChunk<'a, T> {
    pub phys_addr: usize,
    pub slice: &'a mut [T],
}

impl DmaSlice for Dma<u8> {
    type Item = Self;
    fn chunks(&self, bytes: usize) -> DmaChunks<u8> {
        DmaChunks {
            current_offset: 0,
            chunk_size: bytes,
            dma: self,
        }
    }

    fn slice(&self, index: Range<usize>) -> Self::Item {
        assert!(index.end <= self.size, "Index out of bounds");

        unsafe {
            Self {
                virt: self.virt.add(index.start),
                phys: self.phys + index.start,
                size: (index.end - index.start),
            }
        }
    }
}

impl Index<Range<usize>> for Dma<u8> {
    type Output = [u8];

    fn index(&self, index: Range<usize>) -> &Self::Output {
        assert!(index.end <= self.size, "Index out of bounds");

        unsafe { slice::from_raw_parts(self.virt.add(index.start), index.end - index.start) }
    }
}

impl IndexMut<Range<usize>> for Dma<u8> {
    fn index_mut(&mut self, index: Range<usize>) -> &mut Self::Output {
        assert!(index.end <= self.size, "Index out of bounds");
        unsafe { slice::from_raw_parts_mut(self.virt.add(index.start), index.end - index.start) }
    }
}

impl Index<RangeTo<usize>> for Dma<u8> {
    type Output = [u8];

    fn index(&self, index: RangeTo<usize>) -> &Self::Output {
        &self[0..index.end]
    }
}

impl IndexMut<RangeTo<usize>> for Dma<u8> {
    fn index_mut(&mut self, index: RangeTo<usize>) -> &mut Self::Output {
        &mut self[0..index.end]
    }
}

impl Index<RangeFull> for Dma<u8> {
    type Output = [u8];

    fn index(&self, _: RangeFull) -> &Self::Output {
        &self[0..self.size]
    }
}

impl IndexMut<RangeFull> for Dma<u8> {
    fn index_mut(&mut self, _: RangeFull) -> &mut Self::Output {
        let len = self.size;
        &mut self[0..len]
    }
}

impl<T> Dma<T> {
    /// Allocates DMA Memory on a huge page using an `IOAllocator`
    /// # Arguments
    /// * `size` - The size of the memory to allocate
    /// * `allocator` - The allocator to use
    /// # Errors
    pub fn allocate(size: usize, allocator: &IOAllocator) -> Result<Self, Box<dyn Error>> {
        println!("calling allocate with size: {size}");
        allocator.allocate::<T>(size)
    }

    /// Allocates DMA Memory on a huge page using a specific `NVMe` device
    /// # Arguments
    /// * `size` - The size of the memory to allocate
    /// * `nvme` - The `NVMe` device to use
    /// # Errors
    pub fn allocate_nvme(size: usize, nvme: &NvmeDevice) -> Result<Self, Box<dyn Error>> {
        Self::allocate(size, &nvme.allocator)
    }
}

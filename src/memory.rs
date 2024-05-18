use lazy_static::lazy_static;

use std::slice;
// use std::rc::Rc;
// use std::cell::RefCell;
use crate::vfio::*;
use std::collections::HashMap;
use std::error::Error;
use std::io::{self, Read, Seek};
use std::ops::{Deref, DerefMut, Index, IndexMut, Range, RangeFull, RangeTo};
use std::os::fd::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::{fs, mem, process, ptr};
use crate::uio::Uio;

// from https://www.kernel.org/doc/Documentation/x86/x86_64/mm.txt
pub(crate) const X86_VA_WIDTH: u8 = 47;

const HUGE_PAGE_BITS: u32 = 21;
pub const HUGE_PAGE_SIZE: usize = 1 << HUGE_PAGE_BITS;

// todo iova width?
// pub const IOVA_WIDTH: u8 = X86_VA_WIDTH;
pub const IOVA_WIDTH: u8 = 39;

pub(crate) static HUGEPAGE_ID: AtomicUsize = AtomicUsize::new(0);

pub(crate) static mut vfio: Option<Vfio> = None;
pub(crate) static mut uio: Option<Uio> = None;

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
    type Item = Dma<u8>;
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
            Dma {
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

pub(crate) const MAP_HUGE_2MB: i32 = 0x5400_0000; // 21 << 26

impl<T> Dma<T> {
    /// Allocates DMA Memory on a huge page
    pub fn allocate(size: usize) -> Result<Dma<T>, Box<dyn Error>> {
        let size = if size % HUGE_PAGE_SIZE != 0 {
            ((size >> HUGE_PAGE_BITS) + 1) << HUGE_PAGE_BITS
        } else {
            size
        };

        if get_vfio().is_some() {
            if get_uio().is_some() {
                Err("It seems UIO and VFIO are initialized, this should not be possible".into())
            } else {
                Vfio::allocate::<T>(size)
            }
        } else if get_uio().is_some() {
            Uio::allocate::<T>(size)
        } else {
            Err("UIO/VFIO is not initialized, call NvmeDevice init first!".into())
        }

    }
}

pub fn get_vfio() -> &'static Option<Vfio> {
    unsafe { &vfio }
}

pub fn set_vfio(new_vfio: Vfio) {
    unsafe { vfio = Some(new_vfio) }
}

pub fn get_uio() -> &'static Option<Uio> {
    unsafe { &uio }
}

pub fn set_uio(new_uio: Uio) {
    unsafe { uio = Some(new_uio) }
}

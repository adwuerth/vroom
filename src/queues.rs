use crate::cmd::NvmeCommand;
use crate::mapping::{Mapping, MemoryAccess};
use crate::memory::Dma;
use crate::{Result, PAGESIZE_2MIB};
use std::hint::spin_loop;
use std::mem;

/// `NVMe` spec 4.6
/// Completion queue entry
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default)]
#[repr(C, packed)]
pub struct NvmeCompletion {
    /// Command specific
    pub command_specific: u32,
    /// Reserved
    pub _rsvd: u32,
    // Submission queue head
    pub sq_head: u16,
    // Submission queue ID
    pub sq_id: u16,
    // Command ID
    pub c_id: u16,
    //  Status field
    pub status: u16,
}

/// maximum amount of submission entries on a 2MiB huge page
// pub const QUEUE_LENGTH: usize = 1024;
// pub const QUEUE_LENGTH: usize = 65536;
// pub const QUEUE_LENGTH: usize = PAGESIZE_2MIB / mem::size_of::<NvmeCommand>();
pub const QUEUE_LENGTH: usize = ((PAGESIZE_2MIB / mem::size_of::<NvmeCommand>()) >> 1);
// pub const QUEUE_LENGTH: usize = 64;

// pub const QUEUE_LENGTH: usize = 65536;

// static QUEUE_LENGTH: AtomicUsize =
//     AtomicUsize::new((PAGESIZE_2MIB / mem::size_of::<NvmeCommand>()) >> 1);

/// Submission queue
pub struct SubmissionQueue {
    // TODO: switch to mempool for larger queue
    // commands: Dma<[NvmeCommand; QUEUE_LENGTH]>,
    pub(crate) commands: Dma<u8>,
    pub head: usize,
    pub tail: usize,
    len: usize,
    pub doorbell: usize,
}

impl SubmissionQueue {
    pub fn new(allocator: &MemoryAccess, len: usize, doorbell: usize) -> Result<Self> {
        let commands = allocator.allocate(mem::size_of::<NvmeCommand>() * QUEUE_LENGTH)?;

        Ok(Self {
            commands,
            head: 0,
            tail: 0,
            len: len.min(QUEUE_LENGTH),
            doorbell,
        })
    }

    pub const fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    pub const fn is_full(&self) -> bool {
        self.head == (self.tail + 1) % self.len
    }

    pub fn submit_checked(&mut self, entry: NvmeCommand) -> Option<usize> {
        if self.is_full() {
            None
        } else {
            Some(self.submit(entry))
        }
    }

    // #[inline(always)]
    pub fn submit(&mut self, entry: NvmeCommand) -> usize {
        // println!("SUBMISSION ENTRY: {:?}", entry);
        // self.commands[self.tail] = entry;

        let ptr = self.commands.virt;
        let array_ptr = ptr.cast::<[NvmeCommand; QUEUE_LENGTH]>();
        (unsafe { &mut *array_ptr })[self.tail] = entry;

        self.tail = (self.tail + 1) % self.len;
        self.tail
    }

    pub const fn get_addr(&self) -> usize {
        self.commands.phys
    }
}

/// Completion queue
pub struct CompletionQueue {
    pub(crate) commands: Dma<[NvmeCompletion; QUEUE_LENGTH]>,
    head: usize,
    phase: bool,
    len: usize,
    pub doorbell: usize,
}

// TODO: error handling
impl CompletionQueue {
    pub fn new(allocator: &MemoryAccess, len: usize, doorbell: usize) -> Result<Self> {
        let commands = allocator.allocate(mem::size_of::<NvmeCompletion>() * QUEUE_LENGTH)?;
        Ok(Self {
            commands,
            head: 0,
            phase: true,
            len: len.min(QUEUE_LENGTH),
            doorbell,
        })
    }

    pub fn complete(&mut self) -> Option<(usize, NvmeCompletion, usize)> {
        let entry = &self.commands[self.head];

        if ((entry.status & 1) == 1) == self.phase {
            let prev = self.head;
            self.head = (self.head + 1) % self.len;
            if self.head == 0 {
                self.phase = !self.phase;
            }
            Some((self.head, *entry, prev))
        } else {
            None
        }
    }

    pub fn complete_n(&mut self, commands: usize) -> (usize, NvmeCompletion, usize) {
        let prev = self.head;
        self.head += commands - 1;
        if self.head >= self.len {
            self.phase = !self.phase;
        }
        self.head %= self.len;

        let (head, entry, _) = self.complete_spin();
        (head, entry, prev)
    }

    pub fn complete_spin(&mut self) -> (usize, NvmeCompletion, usize) {
        loop {
            if let Some(val) = self.complete() {
                return val;
            }
            spin_loop();
        }
    }

    pub const fn get_addr(&self) -> usize {
        self.commands.phys
    }
}

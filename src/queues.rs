use crate::cmd::NvmeCommand;
use crate::mapping::{Mapping, MemoryAccess};
use crate::memory::Dma;
use crate::Result;
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

pub const QUEUE_LENGTH: usize = 1024;

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
        let commands = allocator.allocate(mem::size_of::<NvmeCommand>() * len)?;

        Ok(Self {
            commands,
            head: 0,
            tail: 0,
            len,
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
        unsafe {
            self.commands
                .virt
                .cast::<NvmeCommand>()
                .add(self.tail)
                .write(entry);
        }

        self.tail = (self.tail + 1) % self.len;
        self.tail
    }

    pub const fn get_addr(&self) -> usize {
        self.commands.phys
    }
}

/// Completion queue
pub struct CompletionQueue {
    pub(crate) commands: Dma<NvmeCompletion>,
    head: usize,
    phase: bool,
    len: usize,
    pub doorbell: usize,
}

// TODO: error handling
impl CompletionQueue {
    pub fn new(allocator: &MemoryAccess, len: usize, doorbell: usize) -> Result<Self> {
        let commands = allocator.allocate(mem::size_of::<NvmeCompletion>() * len)?;
        Ok(Self {
            commands,
            head: 0,
            phase: true,
            len,
            doorbell,
        })
    }

    pub fn complete(&mut self) -> Option<(usize, NvmeCompletion, usize)> {
        let entry = unsafe { self.commands.virt.add(self.head).read() };

        if ((entry.status & 1) == 1) == self.phase {
            let prev = self.head;
            self.head = (self.head + 1) % self.len;
            if self.head == 0 {
                self.phase = !self.phase;
            }
            Some((self.head, entry, prev))
        } else {
            None
        }
    }

    pub fn complete_n(&mut self, commands: usize) -> (usize, NvmeCompletion, usize) {
        assert!(commands > 0);
        let prev = self.head;

        let (mut head, mut entry, _) = self.complete_spin();
        for _ in 1..commands {
            let (h, e, _) = self.complete_spin();
            head = h;
            if (entry.status >> 1) == 0 {
                entry = e;
            }
        }
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

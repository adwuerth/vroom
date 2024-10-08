use crate::cmd::NvmeCommand;
use crate::mapping::{Mapping, MemoryAccess};
use crate::memory::{Dma, DmaSlice, Pagesize};
use crate::queues::{CompletionQueue, NvmeCompletion, SubmissionQueue, QUEUE_LENGTH};
use crate::Result;
use crate::{PAGESIZE_2MIB, PAGESIZE_4KIB};
use std::collections::HashMap;
use std::hint::spin_loop;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[allow(unused, clippy::upper_case_acronyms)]
#[derive(Copy, Clone, Debug)]
enum NvmeRegs32 {
    VS = 0x8,        // Version
    INTMS = 0xC,     // Interrupt Mask Set
    INTMC = 0x10,    // Interrupt Mask Clear
    CC = 0x14,       // Controller Configuration
    CSTS = 0x1C,     // Controller Status
    NSSR = 0x20,     // NVM Subsystem Reset
    AQA = 0x24,      // Admin Queue Attributes
    CMBLOC = 0x38,   // Contoller Memory Buffer Location
    CMBSZ = 0x3C,    // Controller Memory Buffer Size
    BPINFO = 0x40,   // Boot Partition Info
    BPRSEL = 0x44,   // Boot Partition Read Select
    BPMBL = 0x48,    // Bood Partition Memory Location
    CMBSTS = 0x58,   // Controller Memory Buffer Status
    PMRCAP = 0xE00,  // PMem Capabilities
    PMRCTL = 0xE04,  // PMem Region Control
    PMRSTS = 0xE08,  // PMem Region Status
    PMREBS = 0xE0C,  // PMem Elasticity Buffer Size
    PMRSWTP = 0xE10, // PMem Sustained Write Throughput
}

#[allow(unused, clippy::upper_case_acronyms)]
#[derive(Copy, Clone, Debug)]
enum NvmeRegs64 {
    CAP = 0x0,      // Controller Capabilities
    ASQ = 0x28,     // Admin Submission Queue Base Address
    ACQ = 0x30,     // Admin Completion Queue Base Address
    CMBMSC = 0x50,  // Controller Memory Buffer Space Control
    PMRMSC = 0xE14, // Persistent Memory Buffer Space Control
}

#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug)]
enum NvmeArrayRegs {
    SQyTDBL, // Submission Queue Doorbell
    CQyHDBL, // Completion Queue Doorbell
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
#[allow(unused)]
struct IdentifyNamespaceData {
    pub nsze: u64,
    pub ncap: u64,
    nuse: u64,
    nsfeat: u8,
    pub nlbaf: u8,
    pub flbas: u8,
    mc: u8,
    dpc: u8,
    dps: u8,
    nmic: u8,
    rescap: u8,
    fpi: u8,
    dlfeat: u8,
    nawun: u16,
    nawupf: u16,
    nacwu: u16,
    nabsn: u16,
    nabo: u16,
    nabspf: u16,
    noiob: u16,
    nvmcap: u128,
    npwg: u16,
    npwa: u16,
    npdg: u16,
    npda: u16,
    nows: u16,
    _rsvd1: [u8; 18],
    anagrpid: u32,
    _rsvd2: [u8; 3],
    nsattr: u8,
    nvmsetid: u16,
    endgid: u16,
    nguid: [u8; 16],
    eui64: u64,
    pub lba_format_support: [u32; 16],
    _rsvd3: [u8; 192],
    vendor_specific: [u8; 3712],
}

pub struct NvmeQueuePair {
    pub id: u16,
    pub sub_queue: SubmissionQueue,
    comp_queue: CompletionQueue,
}

unsafe impl Send for NvmeQueuePair {}

impl NvmeQueuePair {
    /// returns amount of requests pushed into submission queue
    pub fn submit_io(&mut self, data: &impl DmaSlice, mut lba: u64, write: bool) -> usize {
        let mut reqs = 0;
        // TODO: contruct PRP list?
        for chunk in data.chunks(2 * 4096) {
            let blocks = (chunk.slice.len() as u64 + 512 - 1) / 512;
            let addr = chunk.phys_addr as u64;
            let bytes = blocks * 512;
            let ptr1 = if bytes <= 4096 {
                0
            } else {
                addr + 4096 // self.page_size
            };

            let entry = if write {
                NvmeCommand::io_write(
                    self.id << 11 | self.sub_queue.tail as u16,
                    1,
                    lba,
                    blocks as u16 - 1,
                    addr,
                    ptr1,
                )
            } else {
                NvmeCommand::io_read(
                    self.id << 11 | self.sub_queue.tail as u16,
                    1,
                    lba,
                    blocks as u16 - 1,
                    addr,
                    ptr1,
                )
            };

            if let Some(tail) = self.sub_queue.submit_checked(entry) {
                unsafe {
                    std::ptr::write_volatile(self.sub_queue.doorbell as *mut u32, tail as u32);
                }
            } else {
                eprintln!("error: {entry:?}");
                eprintln!("queue full");
                return reqs;
            }

            lba += blocks;
            reqs += 1;
        }
        reqs
    }

    // TODO: maybe return result
    ///
    /// # Panics
    pub fn complete_io(&mut self, n: usize) -> Option<u16> {
        assert!(n > 0);
        let (tail, c_entry, _) = self.comp_queue.complete_n(n);
        unsafe {
            std::ptr::write_volatile(self.comp_queue.doorbell as *mut u32, tail as u32);
        }
        self.sub_queue.head = c_entry.sq_head as usize;
        let status = c_entry.status >> 1;
        if status != 0 {
            eprintln!(
                "COMPLETE_IO Status: 0x{:x}, Status Code 0x{:x}, Status Code Type: 0x{:x}",
                status,
                status & 0xFF,
                (status >> 8) & 0x7
            );
            eprintln!("{c_entry:?}");
            return None;
        }
        Some(c_entry.sq_head)
    }

    pub fn quick_poll(&mut self) -> Option<()> {
        if let Some((tail, c_entry, _)) = self.comp_queue.complete() {
            unsafe {
                std::ptr::write_volatile(self.comp_queue.doorbell as *mut u32, tail as u32);
            }
            self.sub_queue.head = c_entry.sq_head as usize;
            let status = c_entry.status >> 1;
            let comp_status = c_entry.status;
            if status != 0 {
                eprintln!(
                    "QUICK_POLL Status: 0x{:x}, Status Code 0x{:x}, Status Code Type: 0x{:x}, ---------------> {}",
                    status,
                    status & 0xFF,
                    (status >> 8) & 0x7,
                    Self::u16_to_variable_bit_chunks(comp_status, &vec![1,1,2,3,8,1])
                );
                eprintln!("{c_entry:?}");
            }
            return Some(());
        }
        None
    }

    fn u16_to_variable_bit_chunks(n: u16, chunk_sizes: &Vec<usize>) -> String {
        let binary_string = format!("{n:016b}");
        let mut chunks = Vec::new();
        let mut pos = 0;
        for &size in chunk_sizes {
            let end = if pos + size > 16 { 16 } else { pos + size };
            let chunk: String = binary_string[pos..end].to_string();
            chunks.push(chunk);
            pos = end;
            if pos >= 16 {
                break;
            }
        }
        chunks.join(" ")
    }

    ///
    /// # Errors
    pub fn quick_poll_result(&mut self) -> Result<Option<()>> {
        if let Some((tail, c_entry, _)) = self.comp_queue.complete() {
            unsafe {
                std::ptr::write_volatile(self.comp_queue.doorbell as *mut u32, tail as u32);
            }
            self.sub_queue.head = c_entry.sq_head as usize;
            let status = c_entry.status >> 1;
            if status != 0 {
                let error_message = format!(
                    "QUICK_POLL Status: 0x{:x}, Status Code 0x{:x}, Status Code Type: 0x{:x}\n{:?}",
                    status,
                    status & 0xFF,
                    (status >> 8) & 0x7,
                    c_entry
                );
                eprintln!("{error_message}");
                return Err(format!("Error: {error_message}",).into());
            }
            return Ok(Some(()));
        }
        Ok(None)
    }
}

#[allow(unused)]
pub struct NvmeDevice {
    pub pci_addr: String,
    addr: *mut u8,
    len: usize,
    // Doorbell stride
    dstrd: u16,
    admin_sq: SubmissionQueue,
    admin_cq: CompletionQueue,
    io_sq: SubmissionQueue,
    io_cq: CompletionQueue,
    buffer: Dma<u8>,           // 2MiB of buffer
    prp_list: Dma<[u64; 512]>, // Address of PRP's, devices doesn't necessarily support 2MiB page sizes; 8 Bytes * 512 = 4096
    pub namespaces: HashMap<u32, NvmeNamespace>,
    pub stats: NvmeStats,
    q_id: u16,
    pub allocator: Box<MemoryAccess>,
}

#[derive(Debug, Clone, Copy)]
pub struct NvmeNamespace {
    pub id: u32,
    pub blocks: u64,
    pub block_size: u64,
}

#[derive(Debug, Clone, Default)]
pub struct NvmeStats {
    pub completions: u64,
    pub submissions: u64,
}

// TODO
unsafe impl Send for NvmeDevice {}

unsafe impl Sync for NvmeDevice {}

static BUFFER_SIZE: AtomicUsize = AtomicUsize::new(PAGESIZE_4KIB);

// currently fixed
const PRP_LIST_SIZE: usize = PAGESIZE_4KIB;

#[allow(unused)]
impl NvmeDevice {
    /// Initialises `NVMe` device
    /// # Arguments
    /// * `pci_addr` - pci address of the device
    /// # Errors
    pub fn init(pci_addr: &str, allocator: Box<MemoryAccess>) -> Result<Self> {
        // let allocator: IOAllocator = IOAllocator::init(pci_addr)?;

        // Map the device's BAR
        let (addr, len) = allocator.map_resource()?;

        let buffer: Dma<u8> = allocator.allocate(BUFFER_SIZE.load(Ordering::Relaxed))?;
        let prp_list: Dma<[u64; 512]> = allocator.allocate(PRP_LIST_SIZE)?;

        let mut dev = Self {
            pci_addr: pci_addr.to_string(),
            addr,
            dstrd: {
                unsafe {
                    ((std::ptr::read_volatile(
                        (addr as usize + NvmeRegs64::CAP as usize) as *const u64,
                    ) >> 32)
                        & 0b1111) as u16
                }
            },
            len,
            admin_sq: SubmissionQueue::new(&allocator, QUEUE_LENGTH, 0)?,
            admin_cq: CompletionQueue::new(&allocator, QUEUE_LENGTH, 0)?,
            io_sq: SubmissionQueue::new(&allocator, QUEUE_LENGTH, 0)?,
            io_cq: CompletionQueue::new(&allocator, QUEUE_LENGTH, 0)?,
            buffer,
            prp_list,
            namespaces: HashMap::new(),
            stats: NvmeStats::default(),
            q_id: 1,
            allocator,
        };

        for i in 1..512 {
            dev.prp_list[i - 1] = (dev.buffer.phys + i * 4096) as u64;
        }

        let cap = dev.get_reg64(NvmeRegs64::CAP as u64);
        let maximum_queue_size = (cap & 0xFFFF) as u16 + 1;
        println!("Maximum Queue Size: {maximum_queue_size}");

        println!("CAP: 0x{:x}", dev.get_reg64(NvmeRegs64::CAP as u64));
        println!("VS: 0x{:x}", dev.get_reg32(NvmeRegs32::VS as u32));
        println!("CC: 0x{:x}", dev.get_reg32(NvmeRegs32::CC as u32));

        // Set Enable bit to 0
        let ctrl_config = dev.get_reg32(NvmeRegs32::CC as u32) & 0xFFFF_FFFE;
        dev.set_reg32(NvmeRegs32::CC as u32, ctrl_config);

        // Wait for not ready
        loop {
            let csts = dev.get_reg32(NvmeRegs32::CSTS as u32);
            if csts & 1 == 1 {
                spin_loop();
            } else {
                break;
            }
        }

        // Configure Admin Queues
        // Initialize the addresses of the admin completion/submission queues on the device
        dev.set_reg64(NvmeRegs64::ASQ as u32, dev.admin_sq.get_addr() as u64);
        dev.set_reg64(NvmeRegs64::ACQ as u32, dev.admin_cq.get_addr() as u64);
        dev.set_reg32(
            NvmeRegs32::AQA as u32,
            (QUEUE_LENGTH as u32 - 1) << 16 | (QUEUE_LENGTH as u32 - 1),
        );

        // Configure other stuff
        // TODO: check css values
        let mut cc = dev.get_reg32(NvmeRegs32::CC as u32);
        // mask out reserved stuff
        cc &= 0xFF00_000F;
        // Set Completion (2^4 = 16 Bytes) and Submission Entry (2^6 = 64 Bytes) sizes
        cc |= (4 << 20) | (6 << 16);

        // Set Memory Page Size
        // let mpsmax = ((dev.get_reg64(NvmeRegs64::CAP as u64) >> 52) & 0xF) as u32;
        // cc |= (mpsmax << 7);
        // println!("MPS {}", (cc >> 7) & 0xF);
        dev.set_reg32(NvmeRegs32::CC as u32, cc);

        // Enable the controller
        let ctrl_config = dev.get_reg32(NvmeRegs32::CC as u32) | 1;
        dev.set_reg32(NvmeRegs32::CC as u32, ctrl_config);

        // wait for ready
        loop {
            let csts = dev.get_reg32(NvmeRegs32::CSTS as u32);
            if csts & 1 == 0 {
                spin_loop();
            } else {
                break;
            }
        }

        let q_id = dev.q_id;
        let addr = dev.io_cq.get_addr();
        println!("Requesting i/o completion queue");
        let comp = dev.submit_and_complete_admin(|c_id, _| {
            NvmeCommand::create_io_completion_queue(c_id, q_id, addr, (QUEUE_LENGTH - 1) as u16)
        })?;
        let addr = dev.io_sq.get_addr();
        println!("Requesting i/o submission queue");
        let comp = dev.submit_and_complete_admin(|c_id, _| {
            NvmeCommand::create_io_submission_queue(
                c_id,
                q_id,
                addr,
                (QUEUE_LENGTH - 1) as u16,
                q_id,
            )
        })?;
        dev.q_id += 1;

        Ok(dev)
    }

    /// Identify `NVMe` Controller
    /// # Errors    
    pub fn identify_controller_print(&mut self) -> Result<()> {
        println!("Trying to identify controller");

        let (model, serial, firmware) = self.identify_controller()?;

        println!("  - Model: {model} Serial: {serial} Firmware: {firmware}");

        Ok(())
    }

    /// Identify `NVMe` Controller
    /// # Errors    
    pub fn identify_controller(&mut self) -> Result<(String, String, String)> {
        self.submit_and_complete_admin(NvmeCommand::identify_controller)?;
        let mut serial = String::new();
        let data = &self.buffer;

        for &b in &data[4..24] {
            if b == 0 {
                break;
            }
            serial.push(b as char);
        }

        let mut model = String::new();
        for &b in &data[24..64] {
            if b == 0 {
                break;
            }
            model.push(b as char);
        }

        let mut firmware = String::new();
        for &b in &data[64..72] {
            if b == 0 {
                break;
            }
            firmware.push(b as char);
        }

        let model = model.trim().to_string();
        let serial = serial.trim().to_string();
        let firmware = firmware.trim().to_string();

        Ok((model, serial, firmware))
    }

    // 1 to 1 Submission/Completion Queue Mapping
    ///
    /// # Panics
    /// # Errors
    pub fn create_io_queue_pair(&mut self, len: usize) -> Result<NvmeQueuePair> {
        let q_id = self.q_id;
        // println!("Requesting i/o queue pair with id {q_id}");

        let offset = 0x1000 + ((4 << self.dstrd) * (2 * q_id + 1) as usize);
        assert!(offset <= self.len - 4, "SQ doorbell offset out of bounds");

        let dbl = self.addr as usize + offset;

        let comp_queue = CompletionQueue::new(&self.allocator, len, dbl)?;
        let comp = self.submit_and_complete_admin(|c_id, _| {
            NvmeCommand::create_io_completion_queue(
                c_id,
                q_id,
                comp_queue.get_addr(),
                (len - 1) as u16,
            )
        })?;

        let dbl = self.addr as usize + 0x1000 + ((4 << self.dstrd) * (2 * q_id) as usize);
        let sub_queue = SubmissionQueue::new(&self.allocator, len, dbl)?;
        let comp = self.submit_and_complete_admin(|c_id, _| {
            NvmeCommand::create_io_submission_queue(
                c_id,
                q_id,
                sub_queue.get_addr(),
                (len - 1) as u16,
                q_id,
            )
        })?;

        self.q_id += 1;
        Ok(NvmeQueuePair {
            id: q_id,
            sub_queue,
            comp_queue,
        })
    }

    /// # Errors
    pub fn delete_io_queue_pair(&mut self, qpair: &NvmeQueuePair) -> Result<()> {
        // println!("Deleting i/o queue pair with id {}", qpair.id);
        self.submit_and_complete_admin(|c_id, _| {
            NvmeCommand::delete_io_submission_queue(c_id, qpair.id)
        })?;
        self.submit_and_complete_admin(|c_id, _| {
            NvmeCommand::delete_io_completion_queue(c_id, qpair.id)
        })?;

        self.deallocate(&qpair.sub_queue.commands)?;
        self.deallocate(&qpair.comp_queue.commands)?;
        Ok(())
    }

    pub fn identify_namespace_list(&mut self, base: u32) -> Vec<u32> {
        self.submit_and_complete_admin(|c_id, addr| {
            NvmeCommand::identify_namespace_list(c_id, addr, base)
        });

        // TODO: idk bout this/don't hardcode len
        let data: &[u32] =
            // unsafe { std::slice::from_raw_parts(self.buffer.virt.as_ptr() as *const u32, 1024) };
            unsafe { std::slice::from_raw_parts(self.buffer.virt as *const u32, 1024) };

        data.iter()
            .copied()
            .take_while(|&id| id != 0)
            .collect::<Vec<u32>>()
    }

    pub fn identify_namespace(&mut self, id: u32) -> NvmeNamespace {
        self.submit_and_complete_admin(|c_id, addr| {
            NvmeCommand::identify_namespace(c_id, addr, id)
        });

        let namespace_data: IdentifyNamespaceData =
            unsafe { *(self.buffer.virt as *const IdentifyNamespaceData) };

        // let namespace_data = unsafe { *tmp_buff.virt };
        let size = namespace_data.nsze;
        let blocks = namespace_data.ncap;

        // figure out block size
        let flba_idx = (namespace_data.flbas & 0xF) as usize;
        let flba_data = (namespace_data.lba_format_support[flba_idx] >> 16) & 0xFF;
        let block_size = if (9..32).contains(&flba_data) {
            1 << flba_data
        } else {
            0
        };

        // TODO: check metadata?
        println!("Namespace {id}, Size: {size}, Blocks: {blocks}, Block size: {block_size}");

        let namespace = NvmeNamespace {
            id,
            blocks,
            block_size,
        };
        self.namespaces.insert(id, namespace);
        namespace
    }

    /// TODO: currently namespace 1 is hardcoded
    /// # Errors
    pub fn write(&mut self, data: &impl DmaSlice, mut lba: u64) -> Result<()> {
        for chunk in data.chunks(2 * 4096) {
            let blocks = (chunk.slice.len() as u64 + 512 - 1) / 512;
            self.namespace_io(1, blocks, lba, chunk.phys_addr as u64, true);
            lba += blocks;
        }

        Ok(())
    }

    /// TODO: currently namespace 1 is hardcoded
    /// # Errors
    pub fn write_prp(
        &mut self,
        data: &impl DmaSlice,
        mut lba: u64,
        write: bool,
    ) -> Result<Duration> {
        let mut total = Duration::ZERO;
        for chunk in data.chunks(128 * 4096) {
            let chunk_len = chunk.slice.len();
            let prp_pages = chunk_len / PAGESIZE_4KIB;
            // println!("received {} prp pages", prp_pages);

            for i in 0..prp_pages {
                self.prp_list[i] = (chunk.phys_addr + i * 4096) as u64;
            }

            let blocks = (chunk.slice.len() as u64 + 512 - 1) / 512;
            let start = Instant::now();
            self.namespace_io(1, blocks, lba, chunk.phys_addr as u64, write);
            let elapsed = start.elapsed();
            total += elapsed;

            println!("latency each: {}", elapsed.as_nanos() / prp_pages as u128);

            lba += blocks;
        }

        Ok(total)
    }

    /// `NVMe` read to `DmaSlice`
    /// # Errors
    pub fn read(&mut self, dest: &impl DmaSlice, mut lba: u64) -> Result<()> {
        // let ns = *self.namespaces.get(&1).unwrap();
        for chunk in dest.chunks(2 * 4096) {
            let blocks = (chunk.slice.len() as u64 + 512 - 1) / 512;
            self.namespace_io(1, blocks, lba, chunk.phys_addr as u64, false);
            lba += blocks;
        }
        Ok(())
    }

    /// # Errors
    /// # Panics
    pub fn write_copied(&mut self, data: &[u8], mut lba: u64) -> Result<()> {
        let ns = *self.namespaces.get(&1).unwrap();
        for chunk in data.chunks(128 * 4096) {
            self.buffer[..chunk.len()].copy_from_slice(chunk);
            let blocks = (chunk.len() as u64 + ns.block_size - 1) / ns.block_size;
            self.namespace_io(1, blocks, lba, self.buffer.phys as u64, true);
            lba += blocks;
        }

        Ok(())
    }

    /// # Errors
    /// # Panics
    pub fn read_copied(&mut self, dest: &mut [u8], mut lba: u64) -> Result<()> {
        let ns = *self.namespaces.get(&1).unwrap();
        for chunk in dest.chunks_mut(128 * 4096) {
            let blocks = (chunk.len() as u64 + ns.block_size - 1) / ns.block_size;
            self.namespace_io(1, blocks, lba, self.buffer.phys as u64, false);
            lba += blocks;
            chunk.copy_from_slice(&self.buffer[..chunk.len()]);
        }
        Ok(())
    }

    fn submit_io(
        &mut self,
        ns: &NvmeNamespace,
        addr: u64,
        blocks: u64,
        lba: u64,
        write: bool,
    ) -> Option<usize> {
        assert!(blocks > 0);
        assert!(blocks <= 0x1_0000);
        let q_id = 1;

        let bytes = blocks * ns.block_size;
        let ptr1 = if bytes <= 4096 {
            0
        } else if bytes <= 8192 {
            addr + 4096 // self.page_size
        } else {
            // idk if this works
            let offset = (addr - self.buffer.phys as u64) / 8;
            self.prp_list.phys as u64 + offset
        };

        let entry = if write {
            NvmeCommand::io_write(
                self.io_sq.tail as u16,
                ns.id,
                lba,
                blocks as u16 - 1,
                addr,
                ptr1,
            )
        } else {
            NvmeCommand::io_read(
                self.io_sq.tail as u16,
                ns.id,
                lba,
                blocks as u16 - 1,
                addr,
                ptr1,
            )
        };
        self.io_sq.submit_checked(entry)
    }

    fn complete_io(&mut self, step: u64) -> Option<u16> {
        let q_id = 1;

        let (tail, c_entry, _) = self.io_cq.complete_n(step as usize);
        self.write_reg_idx(NvmeArrayRegs::CQyHDBL, q_id as u16, tail as u32);

        let status = c_entry.status >> 1;
        if status != 0 {
            eprintln!(
                "Status: 0x{:x}, Status Code 0x{:x}, Status Code Type: 0x{:x}",
                status,
                status & 0xFF,
                (status >> 8) & 0x7
            );
            eprintln!("{c_entry:?}");
            return None;
        }
        self.stats.completions += 1;
        Some(c_entry.sq_head)
    }

    /// # Errors
    /// # Panics
    pub fn batched_write(
        &mut self,
        ns_id: u32,
        data: &[u8],
        mut lba: u64,
        batch_len: u64,
    ) -> Result<()> {
        let ns = *self.namespaces.get(&ns_id).unwrap();
        let block_size = 512;
        let q_id = 1;

        for chunk in data.chunks(PAGESIZE_2MIB) {
            self.buffer[..chunk.len()].copy_from_slice(chunk);
            let tail = self.io_sq.tail;

            let batch_len = std::cmp::min(batch_len, chunk.len() as u64 / block_size);
            let batch_size = chunk.len() as u64 / batch_len;
            let blocks = batch_size / ns.block_size;

            for i in 0..batch_len {
                if let Some(tail) = self.submit_io(
                    &ns,
                    self.buffer.phys as u64 + i * batch_size,
                    blocks,
                    lba,
                    true,
                ) {
                    self.stats.submissions += 1;
                    self.write_reg_idx(NvmeArrayRegs::SQyTDBL, q_id as u16, tail as u32);
                } else {
                    eprintln!("tail: {tail}, batch_len: {batch_len}, batch_size: {batch_size}, blocks: {blocks}");
                }
                lba += blocks;
            }
            self.io_sq.head = self.complete_io(batch_len).unwrap() as usize;
        }

        Ok(())
    }

    /// # Errors
    /// # Panics
    pub fn batched_read(
        &mut self,
        ns_id: u32,
        data: &mut [u8],
        mut lba: u64,
        batch_len: u64,
    ) -> Result<()> {
        let ns = *self.namespaces.get(&ns_id).unwrap();
        let block_size = 512;
        let q_id = 1;

        for chunk in data.chunks_mut(PAGESIZE_2MIB) {
            let tail = self.io_sq.tail;

            let batch_len = std::cmp::min(batch_len, chunk.len() as u64 / block_size);
            let batch_size = chunk.len() as u64 / batch_len;
            let blocks = batch_size / ns.block_size;

            for i in 0..batch_len {
                if let Some(tail) = self.submit_io(
                    &ns,
                    self.buffer.phys as u64 + i * batch_size,
                    blocks,
                    lba,
                    false,
                ) {
                    self.stats.submissions += 1;
                    self.write_reg_idx(NvmeArrayRegs::SQyTDBL, q_id as u16, tail as u32);
                } else {
                    eprintln!("tail: {tail}, batch_len: {batch_len}, batch_size: {batch_size}, blocks: {blocks}");
                }
                lba += blocks;
            }
            self.io_sq.head = self.complete_io(batch_len).unwrap() as usize;
            chunk.copy_from_slice(&self.buffer[..chunk.len()]);
        }
        Ok(())
    }

    fn namespace_io(&mut self, ns_id: u32, blocks: u64, lba: u64, addr: u64, write: bool) {
        assert!(blocks > 0);
        assert!(blocks <= 0x1_0000);

        let q_id = 1;

        let bytes = blocks * 512;
        let ptr1 = if bytes <= 4096 {
            0
        } else if bytes <= 8192 {
            // self.buffer.phys as u64 + 4096 // self.page_size
            addr + 4096 // self.page_size
        } else {
            self.prp_list.phys as u64 + 8
        };

        let entry = if write {
            NvmeCommand::io_write(
                self.io_sq.tail as u16,
                ns_id,
                lba,
                blocks as u16 - 1,
                addr,
                ptr1,
            )
        } else {
            NvmeCommand::io_read(
                self.io_sq.tail as u16,
                ns_id,
                lba,
                blocks as u16 - 1,
                addr,
                ptr1,
            )
        };

        let tail = self.io_sq.submit(entry);
        self.stats.submissions += 1;

        self.write_reg_idx(NvmeArrayRegs::SQyTDBL, q_id as u16, tail as u32);
        self.io_sq.head = self.complete_io(1).unwrap() as usize;
    }

    fn submit_and_complete_admin<F: FnOnce(u16, usize) -> NvmeCommand>(
        &mut self,
        cmd_init: F,
    ) -> Result<NvmeCompletion> {
        let cid = self.admin_sq.tail;
        let tail = self.admin_sq.submit(cmd_init(cid as u16, self.buffer.phys));
        self.write_reg_idx(NvmeArrayRegs::SQyTDBL, 0, tail as u32);
        let (head, entry, _) = self.admin_cq.complete_spin();
        self.write_reg_idx(NvmeArrayRegs::CQyHDBL, 0, head as u32);
        let status = entry.status >> 1;
        if status != 0 {
            eprintln!(
                "Status: 0x{:x}, Status Code 0x{:x}, Status Code Type: 0x{:x}",
                status,
                status & 0xFF,
                (status >> 8) & 0x7
            );
            return Err("Requesting i/o completion queue failed".into());
        }
        Ok(entry)
    }

    /// # Panics
    pub fn format_namespace(&mut self, ns_id: Option<u32>) {
        let ns_id = if let Some(ns_id) = ns_id {
            assert!(self.namespaces.contains_key(&ns_id));
            ns_id
        } else {
            0xFFFF_FFFF
        };
        self.submit_and_complete_admin(|c_id, _| NvmeCommand::format_nvm(c_id, ns_id));
    }

    /// Sets Queue `qid` Tail Doorbell to `val`
    fn write_reg_idx(&self, reg: NvmeArrayRegs, qid: u16, val: u32) {
        match reg {
            NvmeArrayRegs::SQyTDBL => unsafe {
                std::ptr::write_volatile(
                    (self.addr as usize + 0x1000 + ((4 << self.dstrd) * (2 * qid)) as usize)
                        as *mut u32,
                    val,
                );
            },
            NvmeArrayRegs::CQyHDBL => unsafe {
                std::ptr::write_volatile(
                    (self.addr as usize + 0x1000 + ((4 << self.dstrd) * (2 * qid + 1)) as usize)
                        as *mut u32,
                    val,
                );
            },
        }
    }

    /// Sets the register at `self.addr` + `reg` to `value`.
    ///
    /// # Panics
    ///
    /// Panics if `self.addr` + `reg` does not belong to the mapped memory of the pci device.
    fn set_reg32(&self, reg: u32, value: u32) {
        assert!(reg as usize <= self.len - 4, "memory access out of bounds");

        unsafe {
            std::ptr::write_volatile((self.addr as usize + reg as usize) as *mut u32, value);
        }
    }

    /// Returns the register at `self.addr` + `reg`.
    ///
    /// # Panics
    ///
    /// Panics if `self.addr` + `reg` does not belong to the mapped memory of the pci device.
    fn get_reg32(&self, reg: u32) -> u32 {
        assert!(reg as usize <= self.len - 4, "memory access out of bounds");

        unsafe { std::ptr::read_volatile((self.addr as usize + reg as usize) as *mut u32) }
    }

    /// Sets the register at `self.addr` + `reg` to `value`.
    ///
    /// # Panics
    ///
    /// Panics if `self.addr` + `reg` does not belong to the mapped memory of the pci device.
    fn set_reg64(&self, reg: u32, value: u64) {
        assert!(reg as usize <= self.len - 8, "memory access out of bounds");

        unsafe {
            std::ptr::write_volatile((self.addr as usize + reg as usize) as *mut u64, value);
        }
    }

    /// Returns the register at `self.addr` + `reg`.
    ///
    /// # Panics
    ///
    /// Panics if `self.addr` + `reg` does not belong to the mapped memory of the pci device.
    fn get_reg64(&self, reg: u64) -> u64 {
        assert!(reg as usize <= self.len - 8, "memory access out of bounds");

        unsafe { std::ptr::read_volatile((self.addr as usize + reg as usize) as *mut u64) }
    }

    pub fn set_page_size(&mut self, page_size: Pagesize) {
        self.allocator.set_page_size(page_size);
    }
}

impl Mapping for NvmeDevice {
    fn allocate<T>(&self, size: usize) -> Result<Dma<T>> {
        self.allocator.allocate(size)
    }

    fn deallocate<T>(&self, dma: &Dma<T>) -> Result<()> {
        self.allocator.deallocate(dma)
    }

    fn map_resource(&self) -> Result<(*mut u8, usize)> {
        self.allocator.map_resource()
    }
}

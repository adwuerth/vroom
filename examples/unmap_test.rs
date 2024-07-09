use std::error::Error;

use vroom::ioallocator::IOAllocator;
use vroom::memory::*;
use vroom::vfio::Vfio;

use std::fs::{self, File};
use std::io::Write;

use rand::seq::SliceRandom;
use rand::thread_rng;
use std::{env, process};

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
    Vfio::set_pagesize(PAGESIZE_4KIB);
    let mut nvme = vroom::init(&pci_addr)?;

    let allocator = &nvme.allocator;

    let vfio = match allocator.as_ref() {
        IOAllocator::VfioAllocator(vfio) => vfio,
        IOAllocator::MmioAllocator(_) => {
            panic!("")
        }
    };

    let map_output = "outputmap.txt";
    fs::remove_file(map_output).ok();
    let mut map_output = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(map_output)?;

    let unmap_output = "outputunmap.txt";
    fs::remove_file(unmap_output).ok();
    let mut unmap_output = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(unmap_output)?;

    let allocate_output = "outputallocate.txt";
    fs::remove_file(allocate_output).ok();
    let mut allocate_output = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(allocate_output)?;

    let mut allocate_combined_output = "outputallocate_combined.txt";
    fs::remove_file(allocate_combined_output).ok();
    let mut allocate_combined_output = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(allocate_combined_output)?;

    const ITERATIONS: u32 = 2 << 8;
    const UNMAP_FREQUENCY: u32 = 2 << 4;
    const ALLOC_SIZE: usize = PAGESIZE_4KIB;

    // for i in 0..ITERATIONS {
    //     let start_time = std::time::Instant::now();
    //     let ptr = Vfio::allocate_with_pagesize(ALLOC_SIZE);
    //     let elapsed = start_time.elapsed();
    //     let elapsed_alloc = elapsed.as_nanos();
    //     writeln!(allocate_output, "{:?}", elapsed.as_nanos()).unwrap();

    //     // let start_time = std::time::Instant::now();
    //     // let dma = vfio.map_dma::<u8>(ptr, ALLOC_SIZE)?;
    //     // let elapsed = start_time.elapsed();
    //     // writeln!(map_output, "{:?}", elapsed.as_nanos()).unwrap();

    //     let (dma, elapsed) = debug_print_map_dma::<u8>(vfio, ptr, ALLOC_SIZE)?;
    //     writeln!(map_output, "{:?}", elapsed).unwrap();

    //     writeln!(allocate_combined_output, "{:?}", elapsed_alloc + elapsed).unwrap();

    //     dma_vec.push(dma);
    // }

    let ptr_vec = allocate_vec(ITERATIONS as usize)?;
    let dma_vec = map_vec(vfio, ptr_vec, ALLOC_SIZE, &mut map_output)?;

    free_dma_vec(vfio, dma_vec, &mut unmap_output)?;

    // let dma = nvme.allocate::<u8>(PAGESIZE_2MIB)?;

    // let start_time = std::time::Instant::now();
    // vfio.unmap_dma(dma)?;
    // println!("Unmapped memory in {:?}", start_time.elapsed());

    // let dma = nvme.allocate::<u8>(PAGESIZE_2MIB)?;

    // let start_time = std::time::Instant::now();
    // vfio.unmap_dma(dma)?;
    // println!("Unmapped memory in {:?}", start_time.elapsed());

    // let dma = nvme.allocate::<u8>(PAGESIZE_2MIB)?;

    // vfio.unmap_dma(dma)?;
    Ok(())
}

fn allocate_vec(size: usize) -> Result<Vec<*mut libc::c_void>, Box<dyn Error>> {
    let mut ptr_vec = Vec::new();

    for _ in 0..size {
        let ptr = Vfio::allocate_with_pagesize(size);
        ptr_vec.push(ptr);
    }

    Ok(ptr_vec)
}

fn map_vec(
    vfio: &Vfio,
    ptr_vec: Vec<*mut libc::c_void>,
    alloc_size: usize,
    map_output: &mut File,
) -> Result<Vec<Dma<u8>>, Box<dyn Error>> {
    let mut dma_vec = Vec::new();

    let mut ptr_vec = ptr_vec;

    ptr_vec.shuffle(&mut thread_rng());

    for ptr in ptr_vec {
        let (dma, elapsed) = debug_print_map_dma::<u8>(vfio, ptr, alloc_size)?;
        writeln!(map_output, "{:?}", elapsed).unwrap();
        dma_vec.push(dma);
    }

    Ok(dma_vec)
}

fn free_dma_vec(
    vfio: &Vfio,
    dma_vec: Vec<Dma<u8>>,
    unmap_output: &mut File,
) -> Result<(), Box<dyn Error>> {
    let mut dma_vec = dma_vec;
    dma_vec.shuffle(&mut thread_rng());

    for dma in dma_vec {
        let start_time = std::time::Instant::now();
        vfio.unmap_dma(dma)?;
        let elapsed = start_time.elapsed();
        writeln!(unmap_output, "{:?}", elapsed.as_nanos()).unwrap();
    }

    Ok(())
}

fn debug_print_map_dma<T>(
    vfio: &Vfio,
    ptr: *mut libc::c_void,
    size: usize,
) -> Result<(Dma<T>, u128), Box<dyn Error>> {
    // let mut output_file = std::fs::OpenOptions::new()
    //     .append(true)
    //     .create(true)
    //     .open("outputmap.txt")?;
    let start = std::time::Instant::now();
    let res = vfio.map_dma(ptr, size)?;
    let duration = start.elapsed();

    println!(
        "ptr: {:x} size: {:?} 4kb: {} 2mb: {} dur: {:?}",
        ptr as usize,
        res.size,
        format_4kib_ptr(ptr),
        format_2mib_ptr(ptr),
        duration.as_nanos()
    );

    // writeln!(output_file, "{:?}", duration.as_nanos())?;

    Ok((res, duration.as_nanos()))
}

fn format_4kib_ptr(ptr: *mut libc::c_void) -> String {
    let pointer_as_usize = ptr as usize;

    let padded_binary_string = format!("{pointer_as_usize:048b}");

    let trimmed_binary_string = &padded_binary_string[padded_binary_string.len() - 48..];

    let chunk_sizes = [9, 9, 9, 9, 12];
    let mut chunks = Vec::new();
    let mut start = 0;

    for &size in &chunk_sizes {
        let end = start + size;
        chunks.push(&trimmed_binary_string[start..end]);
        start = end;
    }

    chunks.join(" ")
}

fn format_2mib_ptr(ptr: *mut libc::c_void) -> String {
    let pointer_as_usize = ptr as usize;

    let padded_binary_string = format!("{pointer_as_usize:048b}");

    let trimmed_binary_string = &padded_binary_string[padded_binary_string.len() - 48..];

    let chunk_sizes = [9, 9, 9, 21];
    let mut chunks = Vec::new();
    let mut start = 0;

    for &size in &chunk_sizes {
        let end = start + size;
        chunks.push(&trimmed_binary_string[start..end]);
        start = end;
    }

    chunks.join(" ")
}

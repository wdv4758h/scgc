//! Simple Conservative Garbage Collector
//!
//! As conservative GC doesn't have precise pointer information,
//! we can't support "moving" feature.

#![feature(alloc, heap_api, oom)]
#![no_std]


extern crate alloc;
#[macro_use]
extern crate log;


use core::mem;


#[derive(Debug)]
#[repr(C)]
pub struct Gc {
    heap_begin: *const u8,
    heap_free: *const u8,
    heap_end: *const u8,
    heap_size: usize,
    stack_begin: Option<*const u8>,
    stack_end: Option<*const u8>,
    record_count: usize,
}

#[derive(Debug)]
#[repr(C)]
struct Record {
    addr: *const u8,
    size: usize,
    status: RecordStatus,
}

#[derive(Debug, PartialEq)]
#[repr(C)]
enum RecordStatus {
    Unknown,
    Touched,
    Referred,
    Deallocated,
}


impl Gc {
    pub fn new(size: usize) -> Gc {
        info!("Setting up GC with {} bytes memory", size);
        let raw = unsafe { alloc::heap::allocate(size, 8) };
        if raw.is_null() {
            alloc::oom();
        }
        info!("Available memory address {:p} ~ {:p}", raw, unsafe { raw.offset(size as isize) });
        Gc {
            heap_begin: raw,
            heap_free: raw,
            heap_end: unsafe { raw.offset(size as isize) },
            heap_size: size,
            stack_begin: None,
            stack_end: None,
            record_count: 0,
        }
    }

    pub fn stack_begin<T>(&mut self, addr: &T) -> &Self {
        self.stack_begin = Some(addr as *const T as *const u8);
        info!("Setting Stack begin to {:?}", self.stack_begin);
        self
    }

    pub fn stack_end<T>(&mut self, addr: &T) -> &Self {
        self.stack_end = Some(addr as *const T as *const u8);
        info!("Setting Stack end to {:?}", self.stack_end);
        self
    }

    /// GC cleanup
    pub fn cleanup(&mut self) {
        info!("Start cleanup");
        self.inner_cleanup();
        info!("End cleanup");
    }

    fn inner_cleanup(&mut self) {
        // Initial
        info!("Initialize Record status");
        let record_size = mem::size_of::<Record>();
        for index in 1..self.record_count+1 {
            let raw = unsafe { self.heap_end.offset(-((index*record_size) as isize)) };
            let mut record = unsafe { mem::transmute::<_, &mut Record>(raw) };
            if record.status != RecordStatus::Deallocated {
                record.status = RecordStatus::Unknown;
            }
        }

        // Mark
        info!("Start the Mark Phase");
        self.scan_touch(self.stack_begin.unwrap(), self.stack_end.unwrap());

        let mut has_touched_record = true;
        while has_touched_record {
            has_touched_record = false;
            for record in
                (1..self.record_count+1)
                    .map(|i| unsafe { self.heap_end.offset(-((i*record_size) as isize)) })
                    .map(|raw| unsafe { mem::transmute::<_, &mut Record>(raw) })
                    .filter(|r| r.status == RecordStatus::Touched) {
                record.status = RecordStatus::Referred;
                has_touched_record = true;
                self.scan_touch(record.addr, unsafe { record.addr.offset(record.size as isize) });
            }
        }

        // Sweep
        info!("Start the Sweep Phase");
        for record in
            (1..self.record_count+1)
                .map(|i| unsafe { self.heap_end.offset(-((i*record_size) as isize)) })
                .map(|raw| unsafe { mem::transmute::<_, &mut Record>(raw) })
                .filter(|r| r.status == RecordStatus::Unknown) {
            self.free_record(record);
        }
    }

    /// allocate raw memory under GC's contronl
    pub fn malloc(&mut self, size: usize) -> Option<*const u8> {
        info!("Try to allocate memory");
        let record_size = mem::size_of::<Record>();

        // from free memory
        if (size + record_size) <=
            (self.heap_end as usize -
             self.heap_free as usize -
             record_size * self.record_count) {

            let result = self.heap_free;
            self.heap_free = unsafe { self.heap_free.offset(size as isize) };
            self.record_count += 1;
            let raw = unsafe { self.heap_end.offset(-((self.record_count*record_size) as isize)) };
            let mut record = unsafe { mem::transmute::<_, &mut Record>(raw) };
            info!("Record: {:p}", record);
            record.addr = result;
            record.size = size;
            record.status = RecordStatus::Referred;
            info!("Allocate from free, {:?}", record);
            return Some(result);
        }

        // from deallocated memory
        let result = self.malloc_from_deallocated(size);
        if result.is_some() {
            info!("Allocate from deallocated, {:?}", result.unwrap());
            return result;
        }

        // try to cleanup
        self.cleanup();
        let result = self.malloc_from_deallocated(size);
        if result.is_some() {
            info!("Allocate from deallocated after cleanup, {:?}", result.unwrap());
        } else {
            info!("No memory after cleanup :(");
        }
        return result;
    }

    fn malloc_from_deallocated(&self, size: usize) -> Option<*const u8> {
        let record_size = mem::size_of::<Record>();
        let record = (1..self.record_count+1)
            .map(|i| unsafe { self.heap_end.offset(-((i*record_size) as isize)) })
            .map(|raw| unsafe { mem::transmute::<_, &mut Record>(raw) })
            .filter(|r| r.status == RecordStatus::Deallocated && r.size >= size)
            .take(1)
            .next();
        if let Some(r) = record {
            r.status = RecordStatus::Referred;
            return Some(r.addr);
        }
        None
    }

    /// Try to use arbitrary memory address to find corresponding GC Record
    fn find_record(&self, addr: *const u8) -> Option<&mut Record> {
        // check memory address is in GC's controlled range
        if !((self.heap_begin as usize <= addr as usize) &&
             (addr as usize <= self.heap_end as usize)) {
            return None;
        }

        info!("Finding Record of address {:?}", addr);
        let record_size = mem::size_of::<Record>();

        let mut start = 0;
        let mut end = self.record_count;
        while end - start > 1 {
            let mid = (start + end) / 2;
            let raw = unsafe { self.heap_end.offset(-((mid*record_size) as isize)) };
            let record = unsafe { mem::transmute::<_, &mut Record>(raw) };
            if addr as usize >= record.addr as usize {
                if unsafe { record.addr.offset(record.size as isize) } as usize > addr as usize {
                    return Some(record);
                } else {
                    start = mid;
                }
            } else {
                end = mid;
            }
        }

        let raw = unsafe { self.heap_end.offset(-((start*record_size) as isize)) };
        let record = unsafe { mem::transmute::<_, &mut Record>(raw) };
        Some(record)
    }

    fn scan_touch(&self, begin: *const u8, end: *const u8) {
        info!("Marking from {:p} to {:p}", begin, end);
        for record in
            (begin as usize..end as usize)
                .map(|ptr| unsafe { *(ptr as *const *const u8) })
                .filter_map(|x| self.find_record(x))
                .filter(|r| r.status == RecordStatus::Unknown) {
            record.status = RecordStatus::Touched;
        }
    }

    /// deallocate GC's record
    fn free_record(&self, record: &mut Record) {
        record.status = RecordStatus::Deallocated;
        info!("Deallocated: {:?}", record);
        // TODO: clean to zero
    }
}

#[macro_export]
macro_rules! malloc {
    ($gc:ident, $size:expr) => ({
        use std::mem;
        let foo = false;
        $gc.stack_end(&foo);
        unsafe { mem::transmute($gc.malloc(4096).unwrap()) }
    })
}

#[macro_export]
macro_rules! malloc_core {
    ($gc:ident, $size:expr) => ({
        use core::mem;
        let foo = false;
        $gc.stack_end(&foo);
        unsafe { mem::transmute($gc.malloc(4096).unwrap()) }
    })
}

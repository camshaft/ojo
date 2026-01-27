// RSEQ-based per-CPU event collection for minimal overhead
//
// This module implements a per-CPU buffer strategy using Linux's Restartable Sequences (RSEQ)
// to avoid contention when recording events from multiple threads. Each CPU core has its own
// buffer, and threads write to their current CPU's buffer without locks.
//
// Based on the implementation from s2n-quic-dc-metrics/src/rseq.rs

use std::{
    cell::Cell,
    ffi::CStr,
    fs,
    mem::MaybeUninit,
    ptr::NonNull,
    sync::{
        atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering},
        Mutex,
    },
};

use crate::EventRecord;

// Number of event slots per page (leaving space for metadata)
const SLOTS: usize = 1024 * 8 - 1;

/// A page of event records for a single CPU
#[repr(C, align(128))]
struct Page {
    // Assembly assumes slots is at index 0
    slots: [MaybeUninit<EventRecord>; SLOTS],
    length: AtomicU64,
}

impl Page {
    fn new() -> Box<Page> {
        Box::new(Page {
            slots: [const { MaybeUninit::uninit() }; SLOTS],
            length: AtomicU64::new(0),
        })
    }
}

fn possible_cpus() -> usize {
    let Ok(content) = fs::read_to_string("/sys/devices/system/cpu/possible") else {
        // As a fallback, ask Rust to provide us how much parallelism we have
        if let Ok(parallelism) = std::thread::available_parallelism() {
            return parallelism.get();
        } else {
            static PRINTED_WARNING: AtomicBool = AtomicBool::new(false);
            if !PRINTED_WARNING.swap(true, Ordering::Relaxed) {
                eprintln!("failed to identify CPU count, falling back to 4 fast CPUs");
            }
            return 4;
        }
    };

    let max_cpu = content
        .trim()
        .split(',')
        .map(|range| {
            if let Some((_start, end)) = range.split_once('-') {
                end.parse::<usize>().unwrap_or(0)
            } else {
                range.parse::<usize>().unwrap_or(0)
            }
        })
        .max()
        .unwrap_or(0);

    // CPU numbering is zero-indexed, so max_cpu + 1 gives us the total count
    max_cpu + 1
}

fn init_per_cpu() -> Box<[AtomicPtr<Page>]> {
    (0..=possible_cpus())
        .map(|_| AtomicPtr::new(std::ptr::null_mut()))
        .collect()
}

/// Per-CPU event collector using RSEQ
pub struct RseqCollector {
    per_cpu: Box<[AtomicPtr<Page>]>,
    
    /// If true, RSEQ is not available and we must use fallback
    must_use_fallback: bool,
    
    /// Fallback queue for when RSEQ fails or is unavailable
    fallback: parking_lot::RwLock<crossbeam_queue::SegQueue<EventRecord>>,
    
    /// Pool of empty pages for reuse
    empty_pages: crossbeam_queue::SegQueue<Box<Page>>,
    
    /// Aggregate storage for collected events
    aggregate: Mutex<Vec<EventRecord>>,
    
    /// Counter for dropped events
    dropped_count: AtomicU64,
}

unsafe impl Send for RseqCollector {}
unsafe impl Sync for RseqCollector {}

static PRINTED_MEMBARRIER_WARNING: AtomicBool = AtomicBool::new(false);

impl RseqCollector {
    #[cfg_attr(not(target_os = "linux"), allow(unused_assignments))]
    pub fn new() -> Self {
        let mut must_use_fallback = false;

        #[cfg(target_os = "linux")]
        {
            let ret = unsafe {
                libc::syscall(
                    libc::SYS_membarrier,
                    libc::MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED,
                    0,
                )
            };
            if ret != 0 {
                if !PRINTED_MEMBARRIER_WARNING.swap(true, Ordering::Relaxed) {
                    eprintln!(
                        "failed to register membarrier: {:?}, {:?}",
                        ret,
                        std::io::Error::last_os_error()
                    );
                }
                must_use_fallback = true;
            }

            #[cfg(target_arch = "aarch64")]
            if !std::arch::is_aarch64_feature_detected!("lse") {
                must_use_fallback = true;
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            must_use_fallback = true;
        }

        RseqCollector {
            must_use_fallback,
            per_cpu: init_per_cpu(),
            fallback: Default::default(),
            empty_pages: Default::default(),
            aggregate: Mutex::new(Vec::new()),
            dropped_count: AtomicU64::new(0),
        }
    }

    /// Record an event
    #[cfg(not(target_os = "linux"))]
    pub fn record(&self, event: EventRecord) {
        self.fallback_push(event)
    }

    /// Record an event using RSEQ
    #[cfg(target_os = "linux")]
    pub fn record(&self, event: EventRecord) {
        if self.must_use_fallback {
            return self.fallback_push(event);
        }

        let rseq_ptr = rseq();
        self.record_inner(event, rseq_ptr);
    }

    #[cfg(target_os = "linux")]
    fn record_inner(&self, event: EventRecord, rseq_ptr: NonNull<Rseq>) {
        unsafe {
            #[cfg(target_arch = "x86_64")]
            std::arch::asm!(
                "
                .pushsection __rseq_cs, \"aw\"
                .balign 32
                9:
                .long 0
                .long 0
                .quad 2f
                .quad (6f-2f)
                .quad 7f
                .popsection

                jmp 7f
                .long {RSEQ_SIG}
                7:
                mov {cpu_id:e}, [{rseq_ptr}+{cpu_id_offset_start}]

                cmp {cpu_id}, {per_cpu_len}
                jge {fallback}

                dec {loop_count}
                jz {fallback}

                lea {tmp}, [rip+9b]
                mov [{rseq_ptr}+{rseq_cs_offset}], {tmp}

                2:
                cmp {cpu_id:e}, [{rseq_ptr}+{cpu_id_offset}]
                jnz 7b

                mov {page_ptr}, [{per_cpu_base}+{cpu_id}*8]
                test {page_ptr}, {page_ptr}
                jz {needs_new_page}

                mov {tmp}, [{page_ptr}+{length_offset}]
                cmp {tmp}, {SLOTS}
                jge {needs_new_page}

                // Calculate slot address: page_ptr + tmp * 40
                mov {slot_ptr}, {tmp}
                imul {slot_ptr}, {RECORD_SIZE}
                add {slot_ptr}, {page_ptr}
                
                // Copy the event record (5 u64 fields = 40 bytes)
                mov {val}, [{event_ptr}]
                mov [{slot_ptr}], {val}
                mov {val}, [{event_ptr}+8]
                mov [{slot_ptr}+8], {val}
                mov {val}, [{event_ptr}+16]
                mov [{slot_ptr}+16], {val}
                mov {val}, [{event_ptr}+24]
                mov [{slot_ptr}+24], {val}
                mov {val}, [{event_ptr}+32]
                mov [{slot_ptr}+32], {val}

                inc {tmp}
                mov [{page_ptr}+{length_offset}], {tmp}
                6:

                mov QWORD PTR [{rseq_ptr}+{rseq_cs_offset}], 0
                ",
                rseq_ptr = in(reg) rseq_ptr.as_ptr(),
                cpu_id = out(reg) _,
                page_ptr = out(reg) _,
                slot_ptr = out(reg) _,
                tmp = out(reg) _,
                val = out(reg) _,
                loop_count = inout(reg) 5u64 => _,
                per_cpu_base = in(reg) self.per_cpu.as_ptr(),
                per_cpu_len = in(reg) self.per_cpu.len(),
                event_ptr = in(reg) &event,
                cpu_id_offset = const std::mem::offset_of!(Rseq, cpu_id),
                cpu_id_offset_start = const std::mem::offset_of!(Rseq, cpu_id_start),
                rseq_cs_offset = const std::mem::offset_of!(Rseq, rseq_cs),
                length_offset = const std::mem::offset_of!(Page, length),
                RSEQ_SIG = const RSEQ_SIG,
                SLOTS = const SLOTS,
                RECORD_SIZE = const std::mem::size_of::<EventRecord>(),
                needs_new_page = label {
                    self.record_slow(rseq_ptr, event);
                },
                fallback = label {
                    self.fallback_push(event);
                },
                options(nostack)
            );

            #[cfg(target_arch = "aarch64")]
            std::arch::asm!(
                "
                .pushsection __rseq_cs, \"aw\"
                .balign 32
                9:
                .long 0
                .long 0
                .quad 2f
                .quad (6f-2f)
                .quad 7f
                .popsection

                b 7f
                .long {RSEQ_SIG}
                7:
                ldr {cpu_id:w}, [{rseq_ptr}, #{cpu_id_offset_start}]

                cmp {cpu_id:w}, {per_cpu_len:w}
                b.ge {fallback}

                subs {loop_count}, {loop_count}, #1
                b.eq {fallback}

                adr {tmp}, 9b
                str {tmp}, [{rseq_ptr}, #{rseq_cs_offset}]

                2:
                ldr {tmp:w}, [{rseq_ptr}, #{cpu_id_offset}]
                cmp {cpu_id:w}, {tmp:w}
                b.ne 7b

                ldr {page_ptr}, [{per_cpu_base}, {cpu_id}, lsl #3]
                cbz {page_ptr}, {needs_new_page}

                ldr {tmp}, [{page_ptr}, {length_offset}]
                cmp {tmp}, {SLOTS}
                b.ge {needs_new_page}

                // Calculate slot address: page_ptr + tmp * 40
                mov {slot_ptr}, {RECORD_SIZE}
                mul {slot_ptr}, {tmp}, {slot_ptr}
                add {slot_ptr}, {slot_ptr}, {page_ptr}
                
                // Copy the event record (5 u64 fields)
                ldp {val1}, {val2}, [{event_ptr}]
                stp {val1}, {val2}, [{slot_ptr}]
                ldp {val1}, {val2}, [{event_ptr}, #16]
                stp {val1}, {val2}, [{slot_ptr}, #16]
                ldr {val1}, [{event_ptr}, #32]
                str {val1}, [{slot_ptr}, #32]

                add {tmp}, {tmp}, #1
                str {tmp}, [{page_ptr}, {length_offset}]
                6:
                str xzr, [{rseq_ptr}, #{rseq_cs_offset}]
                ",
                rseq_ptr = in(reg) rseq_ptr.as_ptr(),
                cpu_id = out(reg) _,
                page_ptr = out(reg) _,
                slot_ptr = out(reg) _,
                tmp = out(reg) _,
                val1 = out(reg) _,
                val2 = out(reg) _,
                loop_count = inout(reg) 5u64 => _,
                per_cpu_base = in(reg) self.per_cpu.as_ptr(),
                per_cpu_len = in(reg) self.per_cpu.len(),
                event_ptr = in(reg) &event,
                cpu_id_offset = const std::mem::offset_of!(Rseq, cpu_id),
                cpu_id_offset_start = const std::mem::offset_of!(Rseq, cpu_id_start),
                rseq_cs_offset = const std::mem::offset_of!(Rseq, rseq_cs),
                // length_offset is too large for aarch64 constant offset (Page slots array is large)
                length_offset = in(reg) std::mem::offset_of!(Page, length),
                RSEQ_SIG = const RSEQ_SIG,
                // SLOTS and RECORD_SIZE are too large for aarch64 constant
                SLOTS = in(reg) SLOTS,
                RECORD_SIZE = in(reg) std::mem::size_of::<EventRecord>(),
                needs_new_page = label {
                    #[allow(unused_unsafe)]
                    unsafe {
                        self.record_slow(rseq_ptr, event);
                    }
                },
                fallback = label {
                    self.fallback_push(event);
                },
                options(nostack)
            );
        }
    }

    #[inline(never)]
    fn fallback_push(&self, event: EventRecord) {
        let read_guard = self.fallback.read();
        read_guard.push(event);
        if read_guard.len() < SLOTS * 2 {
            return;
        }
        drop(read_guard);

        self.aggregate_fallback(false);
    }

    fn aggregate_fallback(&self, force: bool) {
        let mut write_guard = self.fallback.write();
        if !force && write_guard.len() < SLOTS * 2 {
            return;
        }
        let taken = std::mem::take(&mut *write_guard);
        drop(write_guard);

        let mut buffer = Vec::with_capacity(SLOTS * 2);
        taken.into_iter().for_each(|e| buffer.push(e));
        let mut aggregate = self.aggregate.lock().unwrap();
        aggregate.extend(buffer);
        drop(aggregate);
    }

    #[cold]
    #[cfg(target_os = "linux")]
    #[cfg_attr(target_arch = "aarch64", target_feature(enable = "lse"))]
    fn record_slow(&self, rseq_ptr: NonNull<Rseq>, event: EventRecord) {
        let mut new_page = self.empty_pages.pop().unwrap_or_else(Page::new);

        new_page.slots[0].write(event);
        new_page.length.store(1, Ordering::Relaxed);

        let mut taken: *mut Page = Box::into_raw(new_page);

        let mut fallback: u8 = 0;
        unsafe {
            #[cfg(target_arch = "x86_64")]
            std::arch::asm!(
                "
                .pushsection __rseq_cs, \"aw\"
                .balign 32
                12:
                .long 0
                .long 0
                .quad 3f
                .quad (7f-3f)
                .quad 8f
                .popsection

                jmp 8f
                .long {RSEQ_SIG}
                8:
                mov {cpu_id:e}, [{rseq_ptr}+{cpu_id_offset_start}]

                cmp {cpu_id}, {per_cpu_len}
                setge {fallback}
                jge 7f

                dec {loop_count}
                setz {fallback}
                jz 7f

                lea {tmp}, [rip+12b]
                mov [{rseq_ptr}+{rseq_cs_offset}], {tmp}

                3:
                cmp {cpu_id:e}, [{rseq_ptr}+{cpu_id_offset}]
                jnz 8b

                xchg {taken}, [{per_cpu_base}+{cpu_id}*8]

                7:
                mov QWORD PTR [{rseq_ptr}+{rseq_cs_offset}], 0
                ",
                rseq_ptr = in(reg) rseq_ptr.as_ptr(),
                cpu_id = out(reg) _,
                tmp = out(reg) _,
                loop_count = inout(reg) 5u64 => _,
                per_cpu_base = in(reg) self.per_cpu.as_ptr(),
                per_cpu_len = in(reg) self.per_cpu.len(),
                taken = inout(reg) taken,
                cpu_id_offset = const std::mem::offset_of!(Rseq, cpu_id),
                cpu_id_offset_start = const std::mem::offset_of!(Rseq, cpu_id_start),
                rseq_cs_offset = const std::mem::offset_of!(Rseq, rseq_cs),
                RSEQ_SIG = const RSEQ_SIG,
                fallback = inout(reg_byte) fallback,
                options(nostack)
            );

            #[cfg(target_arch = "aarch64")]
            std::arch::asm!(
                "
                .pushsection __rseq_cs, \"aw\"
                .balign 32
                12:
                .long 0
                .long 0
                .quad 3f
                .quad (7f-3f)
                .quad 8f
                .popsection

                b 8f
                .long {RSEQ_SIG}
                8:
                ldr {cpu_id:w}, [{rseq_ptr}, #{cpu_id_offset_start}]

                cmp {cpu_id:w}, {per_cpu_len:w}
                cset {fallback:w}, ge
                b.ge 7f

                subs {loop_count}, {loop_count}, #1
                cset {fallback:w}, eq
                b.eq 7f

                adr {tmp}, 12b
                str {tmp}, [{rseq_ptr}, #{rseq_cs_offset}]

                3:
                ldr {tmp2:w}, [{rseq_ptr}, #{cpu_id_offset}]
                cmp {cpu_id:w}, {tmp2:w}
                b.ne 8b

                add {tmp}, {per_cpu_base}, {cpu_id}, lsl #3
                swp {taken}, {taken}, [{tmp}]

                7:
                str xzr, [{rseq_ptr}, #{rseq_cs_offset}]
                ",
                rseq_ptr = in(reg) rseq_ptr.as_ptr(),
                cpu_id = out(reg) _,
                tmp = out(reg) _,
                tmp2 = out(reg) _,
                loop_count = inout(reg) 5u64 => _,
                per_cpu_base = in(reg) self.per_cpu.as_ptr(),
                per_cpu_len = in(reg) self.per_cpu.len(),
                taken = inout(reg) taken,
                cpu_id_offset = const std::mem::offset_of!(Rseq, cpu_id),
                cpu_id_offset_start = const std::mem::offset_of!(Rseq, cpu_id_start),
                rseq_cs_offset = const std::mem::offset_of!(Rseq, rseq_cs),
                RSEQ_SIG = const RSEQ_SIG,
                fallback = inout(reg) fallback,
                options(nostack)
            );
        }
        let fallback = fallback != 0;

        if fallback {
            assert!(!taken.is_null());
            let taken = unsafe { Box::from_raw(taken) };
            self.handle_events(taken);
        } else {
            if taken.is_null() {
                return;
            }
            self.handle_events(unsafe { Box::from_raw(taken) });
        }
    }

    /// Steal pages from all CPUs and aggregate events
    #[cfg(not(target_os = "linux"))]
    pub fn steal_pages(&self) {
        self.aggregate_fallback(true);
    }

    /// Steal pages from all CPUs using membarrier
    #[cfg(target_os = "linux")]
    pub fn steal_pages(&self) {
        if self.must_use_fallback {
            self.aggregate_fallback(true);
            return;
        }

        let pages = self
            .per_cpu
            .iter()
            .map(|cpu| cpu.swap(std::ptr::null_mut(), Ordering::Relaxed))
            .collect::<Vec<_>>();

        let ret = unsafe {
            libc::syscall(
                libc::SYS_membarrier,
                libc::MEMBARRIER_CMD_PRIVATE_EXPEDITED,
                0,
            )
        };
        if ret != 0 {
            eprintln!(
                "failed to membarrier: {:?}, {:?}",
                ret,
                std::io::Error::last_os_error()
            );
            std::process::abort();
        }

        for page in pages {
            if !page.is_null() {
                self.handle_events(unsafe { Box::from_raw(page) });
            }
        }

        self.aggregate_fallback(true);
    }

    fn handle_events(&self, mut page: Box<Page>) {
        let length = *page.length.get_mut() as usize;
        let mut aggregate = self.aggregate.lock().unwrap();
        
        for i in 0..length {
            let event = unsafe { page.slots[i].assume_init() };
            aggregate.push(event);
        }
        
        drop(aggregate);

        *page.length.get_mut() = 0;
        self.empty_pages.push(page);
    }

    /// Read available events using a callback
    pub fn read_events<F>(&self, mut callback: F)
    where
        F: FnMut(&[EventRecord]),
    {
        // First steal all per-CPU pages
        self.steal_pages();

        // Then read from aggregate
        let mut aggregate = self.aggregate.lock().unwrap();
        if !aggregate.is_empty() {
            callback(&aggregate);
            aggregate.clear();
        }
    }

    /// Get and reset the dropped event count
    pub fn take_dropped_count(&self) -> u64 {
        self.dropped_count.swap(0, Ordering::Relaxed)
    }
}

impl Drop for RseqCollector {
    fn drop(&mut self) {
        self.steal_pages();
    }
}

// RSEQ infrastructure

thread_local! {
    static RSEQ: Cell<Option<NonNull<Rseq>>> = const { Cell::new(None) };
    static RSEQ_ALLOC: Cell<Option<RseqStorage>> = const { Cell::new(None) };
}

struct RseqStorage {
    slot: Box<Rseq>,
    registered: bool,
}

impl Drop for RseqStorage {
    fn drop(&mut self) {
        let Some(taken_address) = RSEQ.take() else {
            return;
        };

        if !self.registered {
            return;
        }

        if let Err(e) = sys_rseq(taken_address.as_ptr(), 1i32) {
            eprintln!("failed to deregister rseq on thread death: {e:?}");
        }
    }
}

#[repr(C)]
#[repr(align(32))]
pub(crate) struct Rseq {
    cpu_id_start: u32,
    cpu_id: u32,
    rseq_cs: u64,
    flags: u32,
}

#[cfg(target_os = "linux")]
pub(crate) fn rseq() -> NonNull<Rseq> {
    if let Some(ptr) = RSEQ.get() {
        return ptr;
    }

    rseq_init()
}

#[cfg(target_arch = "x86_64")]
const RSEQ_SIG: u32 = 0x53053053;

#[cfg(target_arch = "aarch64")]
const RSEQ_SIG: u32 = 0xd428bc00;

#[cfg(test)]
static RSEQ_FAILED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "linux")]
#[cold]
fn rseq_init() -> NonNull<Rseq> {
    if let Ok(libc_rseq) = from_libc() {
        let ptr = NonNull::new(libc_rseq).unwrap();
        RSEQ.set(Some(ptr));
        return ptr;
    }

    let mut rseq_ptr = RseqStorage {
        slot: Box::new(Rseq {
            cpu_id_start: 0,
            cpu_id: 0,
            rseq_cs: 0,
            flags: 0,
        }),
        registered: false,
    };

    RSEQ.set(Some(NonNull::new(&raw mut *rseq_ptr.slot).unwrap()));
    RSEQ_ALLOC.set(Some(rseq_ptr));
    let rseq_ptr = RSEQ.get().unwrap();

    match sys_rseq(rseq_ptr.as_ptr(), 0) {
        Ok(()) => {
            RSEQ_ALLOC.with(|c| {
                let mut v = c.take().expect("just set above");
                v.registered = true;
                c.set(Some(v));
            });
        }
        Err(e) => {
            eprintln!("rseq failed to register: {e:?}");
            #[cfg(test)]
            RSEQ_FAILED.store(true, Ordering::Relaxed);
            unsafe {
                (*rseq_ptr.as_ptr()).cpu_id_start = u32::MAX;
            }
        }
    };

    rseq_ptr
}

#[cfg(target_os = "linux")]
fn dlsym(symbol: &CStr) -> std::io::Result<*mut std::ffi::c_void> {
    unsafe {
        let _ = libc::dlerror();
        let address = libc::dlsym(libc::RTLD_DEFAULT, symbol.as_ptr());
        if let Some(ptr) = NonNull::new(libc::dlerror()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "failed to dlsym {symbol:?}: {:?}",
                    std::ffi::CStr::from_ptr(ptr.as_ptr())
                ),
            ));
        }
        Ok(address)
    }
}

#[cfg(target_os = "linux")]
fn thread_plus_offset(offset: libc::ptrdiff_t) -> *mut std::ffi::c_void {
    let output: *mut std::ffi::c_void;
    unsafe {
        #[cfg(target_arch = "aarch64")]
        std::arch::asm!("mrs {output}, tpidr_el0", output = out(reg) output);

        #[cfg(target_arch = "x86_64")]
        std::arch::asm!("mov {output}, fs:0", output = out(reg) output);
    }
    output.wrapping_offset(offset)
}

#[cfg(target_os = "linux")]
fn from_libc() -> std::io::Result<*mut Rseq> {
    let _size = dlsym(c"__rseq_size")?.cast::<u32>();
    let offset = dlsym(c"__rseq_offset")?.cast::<libc::ptrdiff_t>();
    let _flags = dlsym(c"__rseq_flags")?.cast::<u32>();

    Ok(thread_plus_offset(unsafe { offset.read() }).cast())
}

#[cfg(target_os = "linux")]
fn sys_rseq(rseq: *mut Rseq, flags: i32) -> std::io::Result<()> {
    let ret = unsafe {
        libc::syscall(
            libc::SYS_rseq,
            rseq,
            std::mem::size_of::<Rseq>() as libc::c_ulong,
            flags,
            0u32,
        )
    };
    if ret == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

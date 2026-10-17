use std::path::PathBuf;

mod debugger;
mod errors;
mod globalconstants;
mod image;
mod inspect;
mod interpreter;
mod objectmemory;
mod oop;
mod trace;

use crate::{
    globalconstants::{
        CLASS_METHOD_CONTEXT_POINTER, FALSE_POINTER, HEAP_SEGMENT_SIZE, IMAGE_HEADER_BYTES,
        IMAGE_PAGE_SIZE_BYTES, NIL_POINTER, SCHEDULER_ASSOCIATION_POINTER, TRUE_POINTER,
        VALUE_INDEX,
    },
    image::Image,
    interpreter::Interpreter,
    oop::OOP,
};

fn main() {
    //load_real_image("testBinaries/snapshot.im");
    //run_real_image("testBinaries/snapshot.im", 100);
    serve_real_image("testBinaries/snapshot.im", "127.0.0.1:8080");
}

/// Boots the interpreter on an image and serves the debugger until killed.
///
/// # Parameters:
///     * path (&str): The image to run, relative to the crate root
///     * address (&str): What the debugger should bind
fn serve_real_image(path: &str, address: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);

    println!("debugger on http://{}", address);
    debugger::server::Debugger::new(path)
        .expect("load failed")
        .serve(address);
}

/// Boots the interpreter on an image and runs a bounded number of bytecodes,
/// printing one line of trace before each.
///
/// Bounded because the image will walk into something unimplemented long before
/// it finishes anything, and an unbounded loop would bury the interesting part
/// under whatever it does next. What matters is the last few lines before it
/// stops.
///
/// # Parameters:
///     * path (&str): The image to run, relative to the crate root
///     * cycles (usize): How many bytecodes to execute at most
fn run_real_image(path: &str, cycles: usize) {
    let path = PathBuf::from(env! {"CARGO_MANIFEST_DIR"}).join(path);
    let image = Image::load(path.clone()).expect("load failed");
    let memory = Image::parse_into_memory(&image).expect("parse failed");
    let mut interpreter = Interpreter::from_memory(memory);

    println!("Running image {} for {} cycles", path.display(), cycles);
    println!(
        "----------------------------------------------------------------------------------------------------------------------"
    );
    println!(
        " cycle    IP  Receiver Class       Method OOP   Decoded Instruction                      Stack"
    );
    println!(
        "----------------------------------------------------------------------------------------------------------------------"
    );

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for cycle in 0..cycles {
            println!("{}", trace::trace_line(&interpreter, cycle));
            interpreter.step();
        }
    }));

    println!(
        "{}",
        trace::chain_dump(interpreter.memory(), &inspect::sender_chain(&interpreter))
    );

    if outcome.is_err() {
        println!("stopped by a panic; the stack above is where it was")
    }
}

fn load_real_image(path: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);

    let img = Image::load(path).expect("load failed");

    println!("=== Header ===");
    println!("file size:            {} bytes", img.data_len());
    println!("big endian:           {}", img.big_endian());
    println!("last segment:         {}", img.header().last_segment);
    println!(
        "limit in last page:   {:#06x}",
        img.header().limit_in_last_page
    );
    println!(
        "object table length:  {} words",
        img.header().object_table_length
    );
    println!("image type:           {}", img.header().image_type);

    println!("=== Padding Byte Check ===");
    let heap_words = (img.header().last_segment as usize) * HEAP_SEGMENT_SIZE
        + img.header().limit_in_last_page as usize;
    let heap_end = IMAGE_HEADER_BYTES + heap_words * 2;
    let ot_offset = heap_end.div_ceil(IMAGE_PAGE_SIZE_BYTES) * IMAGE_PAGE_SIZE_BYTES;
    let ot_end = ot_offset + (img.header().object_table_length as usize) * 2;

    let mut last_header_data = 0;
    for (idx, &byte) in img.data().iter().enumerate() {
        if byte != 0 {
            last_header_data = idx;
        }
        if idx == IMAGE_HEADER_BYTES - 1 {
            break;
        }
    }
    let header_data_end = last_header_data + 1;

    let header_pad_zero = img.data()[header_data_end..IMAGE_HEADER_BYTES]
        .iter()
        .all(|&b| b == 0);
    let gap_zero = img.data()[heap_end..ot_offset].iter().all(|&b| b == 0);
    let tail_zero = img.data()[ot_end..].iter().all(|&b| b == 0);

    println!(
        "header tail   (bytes   {}..{}):    {}",
        header_data_end,
        IMAGE_HEADER_BYTES,
        if header_pad_zero {
            "all zero"
        } else {
            "NON-ZERO"
        }
    );
    println!(
        "heap→OT gap   ({}..{}):    {}",
        heap_end,
        ot_offset,
        if gap_zero { "all zero" } else { "NON-ZERO" }
    );
    println!(
        "OT→EOF tail   ({}..{}):    {}",
        ot_end,
        img.data_len(),
        if tail_zero { "all zero" } else { "NON-ZERO" }
    );

    let mem = Image::parse_into_memory(&img).expect("parse failed");

    println!("\n=== ObjectMemory ===");
    let total = mem.object_table_len() / 2;
    let mut live = 0usize;
    let mut free = 0usize;
    for i in 0..total {
        let oop = oop::OOP::from_raw((i * 2) as u16);
        if mem.is_free_oop(oop) {
            free += 1
        } else {
            live += 1
        }
    }
    println!("OT entries: {} (live: {}, free: {})", total, live, free);

    println!("\n=== Well-known classes ===");
    for (oop_val, name) in [
        (NIL_POINTER, "nil"),
        (TRUE_POINTER, "true"),
        (FALSE_POINTER, "false"),
        (SCHEDULER_ASSOCIATION_POINTER, "SchedulerAssociation"),
    ] {
        let class_oop = mem.fetch_class_of(OOP::from_raw(oop_val));
        println!(
            "  {:>22} (oop: {:#06x}) -> class {:#06x}",
            name, oop_val, class_oop
        );
    }

    println!("\n=== Scheduler Chain ===");
    let scheduler = mem.fetch_pointer(VALUE_INDEX, OOP::from_raw(SCHEDULER_ASSOCIATION_POINTER));
    let active_process = mem.fetch_pointer(1, OOP::from_raw(scheduler));
    let initial_context = mem.fetch_pointer(1, OOP::from_raw(active_process));

    println!(
        "  ProcessorScheduler:    oop {:#06x}, class: {:#06x}",
        scheduler,
        mem.fetch_class_of(OOP::from_raw(scheduler))
    );
    println!(
        "  active Process:        oop {:#06x}, class: {:#06x}",
        active_process,
        mem.fetch_class_of(OOP::from_raw(active_process))
    );
    println!(
        "  initial MethodContext: oop {:#06x}, class: {:#06x}, expected {:#06x}",
        initial_context,
        mem.fetch_class_of(OOP::from_raw(initial_context)),
        CLASS_METHOD_CONTEXT_POINTER
    );
}

use std::{path::PathBuf, string};

mod errors;
mod globalconstants;
mod image;
mod objectmemory;
mod oop;

use crate::{
    globalconstants::{
        CLASS_METHOD_CONTEXT_POINTER, FALSE_POINTER, NIL_POINTER, SCHEDULER_ASSOCIATION_POINTER,
        TRUE_POINTER,
    },
    image::Image,
    oop::OOP,
};

fn main() {
    load_real_image("testBinaries/snapshot.im");
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
    let scheduler = mem.fetch_pointer(1, OOP::from_raw(SCHEDULER_ASSOCIATION_POINTER));
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

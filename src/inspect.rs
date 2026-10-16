//! Structured views of a running interpreter.
//!
//! Where `trace` renders text, this answers questions with plain structs: what
//! is in the active context, what is on the stack, what chain of senders led
//! here. Plain because the debugger's front end will read them as JSON, and a
//! string built for a terminal would have to be parsed back apart. `trace`
//! formats these for the terminal; the HTTP layer will serialise them. Neither
//! is part of the virtual machine, and nothing here mutates anything.

use crate::{
    globalconstants::{
        BLOCK_ARGUMENT_COUNT_INDEX, HEADER_INDEX, HOME_INDEX, INSTRUCTION_POINTER_INDEX,
        METHOD_INDEX, NIL_POINTER, RECEIVER_INDEX, SENDER_INDEX, STACK_POINTER_INDEX,
        TEMP_FRAME_START,
    },
    interpreter::Interpreter,
    objectmemory::ObjectMemory,
    oop::OOP,
};

/// One context, unpacked.
///
/// The fields a MethodContext and a BlockContext share are read the same way;
/// the ones they do not (`home`, and where `method` and `receiver` come from)
/// are resolved so that a consumer never has to know which kind it is looking
/// at. `is_block` is kept for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextFrame {
    /// The context object itself.
    pub context: OOP,
    /// Whether this is a BlockContext. Its method and receiver are then the
    /// home context's, and its sender field is really its caller.
    pub is_block: bool,
    /// The MethodContext this frame executes in: itself for a MethodContext,
    /// the home for a BlockContext.
    pub home: OOP,
    /// The CompiledMethod whose bytecodes are running.
    pub method: OOP,
    /// The receiver of that method.
    pub receiver: OOP,
    /// Zero relative byte offset into `method` of the next bytecode to run.
    /// `None` when the context has returned: the interpreter nils the stored
    /// instruction pointer on the way out, which is how a block that outlives
    /// its home gets caught trying to return into it.
    pub ip: Option<usize>,
    /// The arguments and temporaries, in declaration order. For a block these
    /// are the block's own arguments only.
    pub temporaries: Vec<OOP>,
    /// The working stack above the temporaries, bottom first, so `last()` is
    /// the top.
    pub stack: Vec<OOP>,
}

/// The context currently executing, read from the interpreter's registers.
///
/// The active context's stored ip and stack pointer are stale: the interpreter
/// keeps them in registers and only writes them back when it leaves the
/// context. So this frame has to be built from `interpreter.ip()` and
/// `interpreter.sp()`, where every other frame is built from the fields the
/// context object holds.
///
/// # Parameters:
///     * interpreter (&Interpreter): The interpreter to read
///
/// # Returns:
///     * ContextFrame, the active context
pub fn active_frame(interpreter: &Interpreter) -> ContextFrame {
    frame_with_registers(
        interpreter.memory(),
        interpreter.active_context(),
        Some(interpreter.ip()),
        interpreter.sp(),
    )
}

/// A context that is not executing, read from the fields it stores.
///
/// The context stores its instruction pointer one relative and its stack
/// pointer relative to the start of the temporary frame, the same two
/// conversions `fetch_context_registers` does when it enters one.
///
/// # Parameters:
///     * memory (&ObjectMemory): The memory holding the context
///     * context (OOP): The context to unpack
///
/// # Returns:
///     * ContextFrame, the context as its stored fields describe it
pub fn stored_frame(memory: &ObjectMemory, context: OOP) -> ContextFrame {
    let stored_sp = OOP::from_raw(memory.fetch_pointer(STACK_POINTER_INDEX, context))
        .integer_value_of()
        .expect("a context stores its stack pointer as a SmallInteger");

    let ip = OOP::from_raw(memory.fetch_pointer(INSTRUCTION_POINTER_INDEX, context))
        .integer_value_of()
        .ok()
        .map(|stored| (stored - 1) as usize);
    let sp = (stored_sp + TEMP_FRAME_START as i16 - 1) as usize;

    frame_with_registers(memory, context, ip, sp)
}

/// Builds a frame from a context and the two registers that may not be in it.
///
/// Both `active_frame` and `stored_frame` end up here once they know where
/// the ip and stack pointer come from. `sp` is the absolute field index of the
/// top of stack, the way the interpreter's register holds it.
///
/// The temporaries are the first `temporary_count` fields of the frame, from
/// the method header for a MethodContext (the count includes the arguments) and
/// from `BLOCK_ARGUMENT_COUNT_INDEX` for a BlockContext. The stack is whatever
/// lies between those and `sp`.
///
/// # Parameters:
///     * memory (&ObjectMemory): The memory holding the context
///     * context (OOP): The context to unpack
///     * ip (usize): Zero relative byte offset of the next bytecode
///     * sp (usize): Absolute field index of the top of stack
///
/// # Returns:
///     * ContextFrame
fn frame_with_registers(
    memory: &ObjectMemory,
    context: OOP,
    ip: Option<usize>,
    sp: usize,
) -> ContextFrame {
    let is_block = is_block_context(memory, context);

    let home = if is_block {
        OOP::from_raw(memory.fetch_pointer(HOME_INDEX, context))
    } else {
        context
    };

    let method = OOP::from_raw(memory.fetch_pointer(METHOD_INDEX, home));
    let receiver = OOP::from_raw(memory.fetch_pointer(RECEIVER_INDEX, home));

    let temporary_count = if is_block {
        OOP::from_raw(memory.fetch_pointer(BLOCK_ARGUMENT_COUNT_INDEX, context))
            .integer_value_of()
            .expect("a BlockContext's argument count is a SmallInteger") as usize
    } else {
        let header = memory.fetch_pointer(HEADER_INDEX, method);
        // this mirrors extractBits: 3 to 7
        ((header >> 8) & 0x1F) as usize
    };

    let start = TEMP_FRAME_START as usize;
    let end = start + temporary_count;

    ContextFrame {
        context,
        is_block,
        home,
        method,
        receiver,
        ip,
        temporaries: (start..end)
            .map(|i| OOP::from_raw(memory.fetch_pointer(i as u16, context)))
            .collect(),
        stack: (end..=sp)
            .map(|i| OOP::from_raw(memory.fetch_pointer(i as u16, context)))
            .collect(),
    }
}

/// Every context from the active one up to the bottom of the stack.
///
/// Follows `SENDER_INDEX` from each context to the next until it reaches nil.
/// For a BlockContext that field is the caller, which is the same thing for
/// this purpose: it is the context that is waiting for this one to return.
///
/// The first frame is the active one, so it comes from the interpreter's
/// registers; every frame after it comes from stored fields.
///
/// # Parameters:
///     * interpreter (&Interpreter): The interpreter to read
///
/// # Returns:
///     * Vec<ContextFrame>, innermost first
pub fn sender_chain(interpreter: &Interpreter) -> Vec<ContextFrame> {
    let memory = interpreter.memory();
    let mut chain = vec![active_frame(interpreter)];

    let mut sender = memory.fetch_pointer(SENDER_INDEX, interpreter.active_context());

    while sender != NIL_POINTER {
        let context = OOP::from_raw(sender);
        chain.push(stored_frame(memory, context));
        sender = memory.fetch_pointer(SENDER_INDEX, context);
    }

    chain
}

/// How many contexts the sender chain holds, counting the active one.
///
/// Follows the same chain `sender_chain` walks, but only counts: it reads one
/// word per context instead of unpacking a frame per context. That matters
/// because the debugger checks the depth after every step when stepping over
/// or out of a call, where building the frames would be the whole cost of the
/// loop.
///
/// # Parameters:
///     * interpreter (&Interpreter): The interpreter to read
///
/// # Returns:
///     * usize, the chain's length, at least 1
pub fn chain_depth(interpreter: &Interpreter) -> usize {
    let memory = interpreter.memory();
    let mut cnt: usize = 1;

    let mut sender = memory.fetch_pointer(SENDER_INDEX, interpreter.active_context());

    while sender != NIL_POINTER {
        cnt += 1;
        let context = OOP::from_raw(sender);
        sender = memory.fetch_pointer(SENDER_INDEX, context);
    }

    cnt
}

/// Whether a context is a BlockContext.
///
/// The interpreter has this test but keeps it private, and this module is a
/// sibling rather than a child. The test is the same: the field that holds a
/// MethodContext's method holds a BlockContext's argument count, which is a
/// SmallInteger, and a CompiledMethod never is.
///
/// # Parameters:
///     * memory (&ObjectMemory): The memory holding the context
///     * context (OOP): The context to test
///
/// # Returns:
///     * bool
pub fn is_block_context(memory: &ObjectMemory, context: OOP) -> bool {
    let ptr = memory.fetch_pointer(BLOCK_ARGUMENT_COUNT_INDEX, context);
    OOP::from_raw(ptr).is_integer_object()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{globalconstants::CLASS_BLOCK_CONTEXT_POINTER, image::Image, trace};

    use super::*;

    fn snapshot_interpreter() -> Interpreter {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testBinaries/snapshot.im");
        let image = Image::load(path).expect("load failed");
        Interpreter::from_memory(Image::parse_into_memory(&image).expect("parse failed"))
    }

    #[test]
    fn the_active_frame_comes_from_the_registers_not_the_stored_fields() {
        let interpreter = snapshot_interpreter();
        let memory = interpreter.memory();

        let active = active_frame(&interpreter);
        let stored = stored_frame(memory, interpreter.active_context());

        // On a freshly loaded image the two agree, because nothing has run
        // yet and the registers were loaded from the stored fields.
        assert_eq!(active, stored);

        // The resume point: the send that tests what snapshotPrimitive
        // answered, with that answer as the one value on the stack.
        assert_eq!(active.ip, Some(143));
        assert_eq!(
            trace::decode(
                memory,
                active.method,
                active.ip.expect("the active context is executing")
            )
            .text,
            "send #isNil (0 args)"
        );
        assert_eq!(active.stack.len(), 1, "one value, the primitive's answer");
        assert!(!active.is_block);
        assert_eq!(active.home, active.context);
    }

    #[test]
    fn temporaries_are_split_from_the_stack_by_the_method_header() {
        let interpreter = snapshot_interpreter();
        let memory = interpreter.memory();
        let frame = active_frame(&interpreter);

        // snapshotAs: 'snapshot' thenQuit: false, with two temporaries of its
        // own. The count comes from the method header and includes the
        // arguments, so all four land in temporaries and none on the stack.
        assert_eq!(frame.temporaries.len(), 4);
        assert_eq!(trace::describe(memory, frame.temporaries[0]), "'snapshot'");
        assert_eq!(trace::describe(memory, frame.temporaries[1]), "false");
    }

    #[test]
    fn the_sender_chain_walks_to_the_scheduler_and_stops() {
        let interpreter = snapshot_interpreter();
        let memory = interpreter.memory();
        let chain = sender_chain(&interpreter);

        // Innermost first, so the active context leads.
        assert_eq!(chain[0], active_frame(&interpreter));

        // Each frame's context is the previous frame's sender, and the last
        // frame's sender is nil, which is what ended the walk.
        for pair in chain.windows(2) {
            assert_eq!(
                memory.fetch_pointer(SENDER_INDEX, pair[0].context),
                pair[1].context.value
            );
        }
        let last = chain
            .last()
            .expect("the chain has at least the active frame");
        assert_eq!(
            memory.fetch_pointer(SENDER_INDEX, last.context),
            NIL_POINTER
        );

        // The image was suspended nine contexts deep, under the two blocks
        // that make up the scheduler's control loop.
        assert_eq!(chain.len(), 9);
        let blocks: Vec<&ContextFrame> = chain.iter().filter(|frame| frame.is_block).collect();
        assert_eq!(blocks.len(), 2);
        for block in blocks {
            assert_eq!(
                memory.fetch_class_of(block.context),
                CLASS_BLOCK_CONTEXT_POINTER
            );
            assert_ne!(
                block.home, block.context,
                "a block's home is another context"
            );
            assert!(
                !is_block_context(memory, block.home),
                "and that one is a MethodContext"
            );
        }
    }

    #[test]
    fn the_depth_is_the_length_of_the_chain() {
        let interpreter = snapshot_interpreter();

        // chain_depth counts what sender_chain builds, one word per context
        // instead of one frame. Tying the two together here is what keeps the
        // cheap walk honest about the expensive one.
        assert_eq!(chain_depth(&interpreter), sender_chain(&interpreter).len());
    }

    #[test]
    fn a_block_frame_takes_its_method_and_receiver_from_its_home() {
        let interpreter = snapshot_interpreter();
        let memory = interpreter.memory();
        let chain = sender_chain(&interpreter);

        let block = chain
            .iter()
            .find(|frame| frame.is_block)
            .expect("the chain holds a block");
        let home = stored_frame(memory, block.home);

        assert_eq!(block.method, home.method);
        assert_eq!(block.receiver, home.receiver);
        // But not its instruction pointer: the block runs its own bytecodes
        // inside the shared method.
        assert_eq!(home.ip, None);
        assert!(block.ip.is_some());
    }
}

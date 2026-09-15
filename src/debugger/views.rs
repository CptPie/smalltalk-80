//! What the front end sees.
//!
//! Every struct here is built from `inspect` and `trace` and derives
//! `Serialize`, and nothing else in the crate knows serde exists. The rule is
//! that an OOP never travels alone: it goes as a `Described`, number and text
//! together, so that a frame's receiver can be shown as `a Rectangle (0x27c4)`
//! and still be followed as a link to `/object/10180`.
//!
//! Anything that can be long is paginated at the endpoint, not here: these are
//! views of one thing each.

use serde::Serialize;

use crate::{
    globalconstants::{CLASS_SMALL_INTEGER_POINTER, HEADER_INDEX, HEADER_SIZE, LITERAL_START},
    inspect::{ContextFrame, active_frame, is_block_context, sender_chain},
    interpreter::Interpreter,
    objectmemory::ObjectMemory,
    oop::OOP,
    trace::{self, byte_object_text, class_name, decode, describe, disassemble, first_bytecode_of},
};

/// An object pointer and how it reads.
///
/// `oop` is the raw value, for links and equality. `text` is what `describe`
/// makes of it: `nil`, `42`, `#at:put:`, `'a string'`, or `a ClassName (0x..)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Described {
    pub oop: u16,
    pub text: String,
}

/// Wraps an OOP with its description.
///
/// # Parameters:
///     * memory (&ObjectMemory): The memory holding the object
///     * oop (OOP): The pointer to describe
///
/// # Returns:
///     * Described
pub fn described(memory: &ObjectMemory, oop: OOP) -> Described {
    Described {
        oop: oop.value,
        text: trace::describe(memory, oop),
    }
}

/// One context, as `inspect::ContextFrame` but with every pointer described
/// and the receiver's class named.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FrameView {
    pub context: Described,
    pub is_block: bool,
    pub home: Described,
    pub method: Described,
    pub receiver: Described,
    /// The receiver's class, by name, since that is what a stack listing
    /// shows in its left column.
    pub receiver_class: String,
    /// `None` when the context has returned.
    pub ip: Option<usize>,
    /// The bytecode at `ip`, decoded, or `None` for a returned context.
    pub instruction: Option<String>,
    pub temporaries: Vec<Described>,
    /// Bottom first, so the last element is the top.
    pub stack: Vec<Described>,
}

/// Builds the view of one frame.
///
/// # Parameters:
///     * memory (&ObjectMemory): The memory holding the frame's objects
///     * frame (&ContextFrame): The frame to view
///
/// # Returns:
///     * FrameView
pub fn frame_view(memory: &ObjectMemory, frame: &ContextFrame) -> FrameView {
    FrameView {
        context: described(memory, frame.context),
        is_block: frame.is_block,
        home: described(memory, frame.home),
        method: described(memory, frame.method),
        receiver: described(memory, frame.receiver),
        receiver_class: class_name(memory, OOP::from_raw(memory.fetch_class_of(frame.receiver))),
        ip: frame.ip,
        instruction: frame.ip.map(|ip| decode(memory, frame.method, ip).text),
        temporaries: frame
            .temporaries
            .iter()
            .map(|oop| described(memory, *oop))
            .collect(),
        stack: frame
            .stack
            .iter()
            .map(|oop| described(memory, *oop))
            .collect(),
    }
}

/// What `/state` answers: where the interpreter is right now, small enough to
/// poll.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateView {
    /// How many bytecodes have run since boot.
    pub cycle: usize,
    /// Whether the control loop is stepping on its own.
    pub running: bool,
    pub active: FrameView,
    /// How deep the sender chain is, so a front end can show it changing
    /// without fetching the whole chain every poll.
    pub depth: usize,
}

/// Builds the state view.
///
/// # Parameters:
///     * interpreter (&Interpreter): The interpreter to describe
///     * cycle (usize): The control loop's cycle count
///     * running (bool): The control loop's run state
///
/// # Returns:
///     * StateView
pub fn state_view(interpreter: &Interpreter, cycle: usize, running: bool) -> StateView {
    StateView {
        cycle,
        running,
        active: frame_view(interpreter.memory(), &active_frame(interpreter)),
        depth: sender_chain(interpreter).len(),
    }
}

/// One decoded bytecode with its offset, for a method listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionView {
    pub ip: usize,
    pub length: usize,
    pub text: String,
}

/// A whole CompiledMethod: its header facts, its literals, and its listing.
///
/// Keyed by the method's OOP and each instruction's `ip`, which is what a
/// source mapping or a JIT's native listing will attach to later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MethodView {
    pub method: Described,
    /// From the header: how many arguments the method takes.
    pub argument_count: usize,
    /// From the header: arguments plus temporaries.
    pub temporary_count: usize,
    /// From the header extension, 0 when the method has none.
    pub primitive_index: usize,
    /// The literal frame, in order, so `push literal 3` can be followed.
    pub literals: Vec<Described>,
    /// Where the bytecodes start, for showing the header and literal frame
    /// as raw words above the listing.
    pub first_bytecode: usize,
    pub instructions: Vec<InstructionView>,
}

/// Builds the view of one method.
///
/// The header fields are read the way the interpreter reads them, but from
/// here rather than through it, since the interpreter keeps those accessors
/// private and a view should not need a running interpreter to build.
///
/// # Parameters:
///     * memory (&ObjectMemory): The memory holding the method
///     * method (OOP): The CompiledMethod to view
///
/// # Returns:
///     * MethodView
pub fn method_view(memory: &ObjectMemory, method: OOP) -> MethodView {
    let header = memory.fetch_word(HEADER_INDEX, method);

    // 3 msb (0..2 in the book terminology)
    let flag = ((header >> 13) & 0x7) as usize;

    // bits 3..7 (the remainder of the high nibble)
    let temporary_count = ((header >> 8) & 0x1F) as usize;

    // bits 9..14 (the 'middle' bits of the low nibble. left shift once, and trim the msb with the
    // mask)
    let literal_count = (header >> 1) & 0x3F;

    let (argument_count, primitive_index) = match flag {
        0..=4 => (flag, 0),
        5 | 6 => (0, 0),
        _ => {
            let extension = memory.fetch_word(LITERAL_START + literal_count - 2, method);
            (
                ((extension >> 9) & 0x1F) as usize,
                ((extension >> 1) & 0xFF) as usize,
            )
        }
    };

    let literals = (0..literal_count)
        .map(|index| {
            described(
                memory,
                OOP::from_raw(memory.fetch_pointer(index + LITERAL_START, method)),
            )
        })
        .collect();

    MethodView {
        method: described(memory, method),
        argument_count,
        temporary_count,
        primitive_index,
        literals,
        first_bytecode: first_bytecode_of(memory, method),
        instructions: disassemble(memory, method)
            .into_iter()
            .map(|(ip, instruction)| InstructionView {
                ip,
                length: instruction.length,
                text: instruction.text,
            })
            .collect(),
    }
}

/// An object's contents, one level deep.
///
/// A pointer object lists its fields, described; a byte object gives its bytes
/// and, when they read as text, the text. The class comes both ways so a front
/// end can show the name and link to the class object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObjectView {
    pub object: Described,
    pub class: Described,
    pub class_name: String,
    /// Size in words including the two header words, as the heap stores it.
    pub size: usize,
    pub is_pointer_object: bool,
    /// The fields of a pointer object, or empty.
    pub fields: Vec<Described>,
    /// The bytes of a byte object, or empty.
    pub bytes: Vec<u8>,
    /// The bytes as Latin-1 text, when this is a byte object.
    pub text: Option<String>,
}

/// Builds the view of one object.
///
/// A SmallInteger has no object behind it; the view for one has the tag's
/// value as its description, the SmallInteger class, a size of zero and no
/// contents.
///
/// # Parameters:
///     * memory (&ObjectMemory): The memory holding the object
///     * oop (OOP): The object to view
///
/// # Returns:
///     * ObjectView
pub fn object_view(memory: &ObjectMemory, oop: OOP) -> ObjectView {
    let object = described(memory, oop);

    if oop.is_integer_object() {
        return ObjectView {
            object,
            class: described(memory, OOP::from_raw(CLASS_SMALL_INTEGER_POINTER)),
            class_name: class_name(memory, OOP::from_raw(CLASS_SMALL_INTEGER_POINTER)),
            size: 0,
            is_pointer_object: false,
            fields: Vec::new(),
            bytes: Vec::new(),
            text: None,
        };
    }

    let class = OOP::from_raw(memory.fetch_class_of(oop));
    let is_pointer_object = memory.pointer_bit_of(oop);

    let (fields, bytes, text) = if is_pointer_object {
        let fields = (0..memory.fetch_word_length_of(oop))
            .map(|index| described(memory, OOP::from_raw(memory.fetch_pointer(index, oop))))
            .collect();
        (fields, Vec::new(), None)
    } else {
        let bytes: Vec<u8> = (0..memory.fetch_byte_length_of(oop))
            .map(|index| memory.fetch_byte(index, oop))
            .collect();
        (Vec::new(), bytes, Some(byte_object_text(memory, oop)))
    };

    ObjectView {
        object,
        class: described(memory, class),
        class_name: class_name(memory, class),
        size: memory.fetch_word_length_of(oop) as usize + HEADER_SIZE as usize,
        is_pointer_object,
        fields,
        bytes,
        text,
    }
}

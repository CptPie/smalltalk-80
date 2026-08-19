//! Rendering a running interpreter as text.
//!
//! Nothing here is part of the virtual machine: it only reads. The point is to
//! make a bytecode stream legible enough that a wrong answer can be traced back
//! to the send that produced it, which no amount of staring at `{:#06x}` will
//! do. The formatting sits apart from the interpreter so that the eventual
//! inspector can reuse it rather than reinvent it.

use crate::{
    globalconstants::{
        CLASS_NAME_INDEX, FALSE_POINTER, HEADER_INDEX, LITERAL_START, NIL_POINTER,
        SPECIAL_SELECTORS_ARRAY_POINTER, TRUE_POINTER,
    },
    interpreter::Interpreter,
    objectmemory::ObjectMemory,
    oop::OOP,
};

/// One decoded bytecode.
pub struct Instruction {
    /// How it reads, operands resolved where they are worth resolving, e.g.
    /// `send #at:put: (2 args)` rather than `send literal 12`.
    pub text: String,
    /// How many bytes it occupies, so a caller can walk to the next one.
    pub length: usize,
}

/// Returns the byte offset of a method's first bytecode.
///
/// A CompiledMethod is a header word, then the literal frame, then the
/// bytecodes, all in one object. The header says how many literals there are,
/// which is the only way to know where the code starts.
///
/// # Parameters:
///     * memory (&ObjectMemory): The memory holding the method
///     * method (OOP): The CompiledMethod to measure
///
/// # Returns:
///     * usize, byte offset into the method of its first bytecode
pub fn first_bytecode_of(memory: &ObjectMemory, method: OOP) -> usize {
    // The literal count is header bits 9 through 14 in the Bluebook's numbering,
    // where bit 0 is the most significant, so it is bits 1 through 6 counting
    // the way a shift does.
    let literal_count = (memory.fetch_pointer(HEADER_INDEX, method) >> 1) & 0x3F;
    ((literal_count + LITERAL_START) * 2) as usize
}

/// Decodes the bytecode at `ip` in `method`.
///
/// Extended bytecodes take their operands from the bytes that follow, so the
/// length is not knowable from the first byte alone and comes back with the
/// text.
///
/// # Parameters:
///     * memory (&ObjectMemory): The memory holding the method
///     * method (OOP): The CompiledMethod to read from
///     * ip (usize): Byte offset into the method of the bytecode to decode
///
/// # Returns:
///     * Instruction, the rendered bytecode and its length in bytes
pub fn decode(memory: &ObjectMemory, method: OOP, ip: usize) -> Instruction {
    let bytecode = memory.fetch_byte(ip as u16, method);
    // Operand bytes follow the bytecode in the method's byte stream.
    let operand = |n: usize| memory.fetch_byte((ip + n) as u16, method);
    // The literal frame sits between the method header and the bytecodes.
    let literal = |index: u16| OOP::from_raw(memory.fetch_pointer(index + LITERAL_START, method));
    // A literal variable is an Association, and its key is the global's name,
    // which is the half worth printing.
    let binding = |index: u16| {
        describe(
            memory,
            OOP::from_raw(memory.fetch_pointer(0, literal(index))),
        )
    };
    // The special selector array pairs each selector with its argument count.
    let special = |index: u16| {
        describe(
            memory,
            OOP::from_raw(
                memory.fetch_pointer(index * 2, OOP::from_raw(SPECIAL_SELECTORS_ARRAY_POINTER)),
            ),
        )
    };

    // The variable type and index an extended push, store or store-and-pop
    // packs into its single descriptor byte: type in the top two bits, index in
    // the remaining six.
    let extended_variable = |descriptor: u8| {
        let index = descriptor & 0x3F;
        match descriptor >> 6 {
            0 => format!("receiver variable {}", index),
            1 => format!("temporary {}", index),
            2 => format!("literal {}", describe(memory, literal(index as u16))),
            _ => format!("binding {}", binding(index as u16)),
        }
    };

    let (text, length) = match bytecode {
        0..=15 => (format!("push receiver variable {}", bytecode), 1),
        16..=31 => (format!("push temporary {}", bytecode - 16), 1),
        32..=63 => (
            format!(
                "push literal {}",
                describe(memory, literal((bytecode - 32) as u16))
            ),
            1,
        ),
        64..=95 => (
            format!("push binding {}", binding((bytecode - 64) as u16)),
            1,
        ),
        96..=103 => (format!("pop into receiver variable {}", bytecode - 96), 1),
        104..=111 => (format!("pop into temporary {}", bytecode - 104), 1),
        112 => ("push self".to_string(), 1),
        113 => ("push true".to_string(), 1),
        114 => ("push false".to_string(), 1),
        115 => ("push nil".to_string(), 1),
        116 => ("push -1".to_string(), 1),
        117 => ("push 0".to_string(), 1),
        118 => ("push 1".to_string(), 1),
        119 => ("push 2".to_string(), 1),

        120 => ("return self".to_string(), 1),
        121 => ("return true".to_string(), 1),
        122 => ("return false".to_string(), 1),
        123 => ("return nil".to_string(), 1),
        124 => ("return top".to_string(), 1),
        125 => ("return top from block".to_string(), 1),

        128 => (format!("push {}", extended_variable(operand(1))), 2),
        129 => (format!("store into {}", extended_variable(operand(1))), 2),
        130 => (format!("pop into {}", extended_variable(operand(1))), 2),

        // Single extended sends pack the argument count into the top three bits
        // of their descriptor and the literal index into the remaining five.
        131 | 133 => {
            let descriptor = operand(1);
            let selector = describe(memory, literal((descriptor & 0x1F) as u16));
            let arguments = descriptor >> 5;
            let super_send = if bytecode == 133 { " to super" } else { "" };
            (
                format!("send {} ({} args){}", selector, arguments, super_send),
                2,
            )
        }
        // The double extended forms spend a whole byte on each, which is what
        // lets them reach past the five bit literal index above.
        132 | 134 => {
            let selector = describe(memory, literal(operand(2) as u16));
            let super_send = if bytecode == 134 { " to super" } else { "" };
            (
                format!("send {} ({} args){}", selector, operand(1), super_send),
                3,
            )
        }

        135 => ("pop".to_string(), 1),
        136 => ("duplicate top".to_string(), 1),
        137 => ("push thisContext".to_string(), 1),

        144..=151 => (format!("jump {}", bytecode as i16 - 144 + 1), 1),
        152..=159 => (format!("jump {} if false", bytecode as i16 - 152 + 1), 1),
        160..=167 => (
            format!(
                "jump {}",
                (bytecode as i16 - 160 - 4) * 256 + operand(1) as i16
            ),
            2,
        ),
        168..=171 => (
            format!(
                "jump {} if true",
                (bytecode as i16 - 168) * 256 + operand(1) as i16
            ),
            2,
        ),
        172..=175 => (
            format!(
                "jump {} if false",
                (bytecode as i16 - 172) * 256 + operand(1) as i16
            ),
            2,
        ),

        // The special selectors are the arithmetic and comparison messages the
        // compiler emits as one byte, looked up in a shared array rather than in
        // the method's own literals.
        176..=207 => {
            let index = (bytecode - 176) as u16;
            let arguments = OOP::from_raw(memory.fetch_pointer(
                index * 2 + 1,
                OOP::from_raw(SPECIAL_SELECTORS_ARRAY_POINTER),
            ));
            (
                format!(
                    "send {} ({} args)",
                    special(index),
                    describe(memory, arguments)
                ),
                1,
            )
        }
        208..=255 => (
            format!(
                "send {} ({} args)",
                describe(memory, literal((bytecode & 0x0F) as u16)),
                (bytecode >> 4) - 13
            ),
            1,
        ),

        // 126, 127 and 138 through 143 are the gaps in the bytecode set. The
        // compiler never emits them, so seeing one means the method is not a
        // method, which is worth saying rather than hiding.
        _ => (format!("unused bytecode {}", bytecode), 1),
    };

    Instruction { text, length }
}

/// Renders an object as something a reader can recognise.
///
/// SmallIntegers, nil, true and false print as themselves; a Symbol or String
/// prints its characters, which is what makes a send legible; anything else
/// prints as its class name and pointer. Recursion stops at one level: a field
/// of an object is described, the fields of that field are not.
///
/// # Parameters:
///     * memory (&ObjectMemory): The memory holding the object
///     * oop (OOP): The object to describe
///
/// # Returns:
///     * String, e.g. `#at:put:`, `42`, `nil`, `a Rectangle (0x27c4)`
pub fn describe(memory: &ObjectMemory, oop: OOP) -> String {
    if oop.is_integer_object() {
        return format!(
            "{}",
            oop.integer_value_of()
                .expect("integer tag is already checked above")
        );
    }

    if oop.value == NIL_POINTER {
        return String::from("nil");
    }

    if oop.value == TRUE_POINTER {
        return String::from("true");
    }

    if oop.value == FALSE_POINTER {
        return String::from("false");
    }

    let name = class_name(memory, OOP::from_raw(memory.fetch_class_of(oop)));
    match name.as_str() {
        "Symbol" => format!("#{}", byte_object_text(memory, oop)),
        "String" => format!("'{}'", byte_object_text(memory, oop)),
        _ => {
            let article = if name.to_lowercase().starts_with(['a', 'e', 'i', 'o', 'u']) {
                "an"
            } else {
                "a"
            };
            format!("{} {} ({:#06x})", article, name, oop.value)
        }
    }
}

/// Reads the characters of a byte object as text.
///
/// # Parameters:
///     * memory (&ObjectMemory): The memory holding the object
///     * oop (OOP): A byte object, typically a Symbol or a String
///
/// # Returns:
///     * String, the bytes read as Latin-1, which is what the image encodes
pub fn byte_object_text(memory: &ObjectMemory, oop: OOP) -> String {
    let len = memory.fetch_byte_length_of(oop);
    let mut text = String::with_capacity(len as usize);
    for i in 0..len {
        let byte = memory.fetch_byte(i, oop);
        let character = char::from(byte);
        text.push(character);
    }
    text
}

/// Returns the name of a class or metaclass.
///
/// A Class holds a Symbol in `CLASS_NAME_INDEX`. A Metaclass holds the class it
/// belongs to at the same index and has no name of its own, so it is named after
/// that class, which is where the `Foo class` spelling comes from.
///
/// # Parameters:
///     * memory (&ObjectMemory): The memory holding the class
///     * class (OOP): The class or metaclass to name
///
/// # Returns:
///     * String, e.g. `Rectangle` or `Rectangle class`
pub fn class_name(memory: &ObjectMemory, class: OOP) -> String {
    let named = OOP::from_raw(memory.fetch_pointer(CLASS_NAME_INDEX, class));

    if memory.pointer_bit_of(named) {
        // field 6 (CLASS_NAME_INDEX) holds either the object's name as bytes, or the pointer to the
        // parent class (determined by the pointer bit)
        format!("{} class", class_name(memory, named))
    } else {
        byte_object_text(memory, named)
    }
}

/// Renders one line of trace for the bytecode the interpreter is about to run.
///
/// Reads the interpreter without advancing it, so it can be called before every
/// `step` and the line always describes what is about to happen rather than what
/// just did.
///
/// # Parameters:
///     * interpreter (&Interpreter): The interpreter to describe
///     * cycle (usize): How many bytecodes have run, for the left margin
///
/// # Returns:
///     * String, one line, no trailing newline
pub fn trace_line(interpreter: &Interpreter, cycle: usize) -> String {
    let memory = interpreter.memory();
    let receiver_class = class_name(
        memory,
        OOP::from_raw(memory.fetch_class_of(interpreter.receiver())),
    );

    format!(
        "{:>6} {:>5}  {:<20} {:<#06x}       {}",
        cycle,
        interpreter.ip(),
        receiver_class,
        interpreter.method().value,
        decode(memory, interpreter.method(), interpreter.ip()).text
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        globalconstants::{
            ACTIVE_PROCESS_INDEX, CLASS_STRING_POINTER, DOES_NOT_UNDERSTAND_SELECTOR,
            INSTRUCTION_POINTER_INDEX, METHOD_INDEX, NIL_POINTER, SCHEDULER_ASSOCIATION_POINTER,
            SUSPENDED_CONTEXT_INDEX, VALUE_INDEX,
        },
        image::Image,
    };

    use super::*;

    fn snapshot() -> ObjectMemory {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testBinaries/snapshot.im");
        let image = Image::load(path).expect("load failed");
        Image::parse_into_memory(&image).expect("parse failed")
    }

    #[test]
    fn decoding_walks_to_the_bytecode_the_image_was_suspended_at() {
        let memory = snapshot();

        // The image resumes in SystemDictionary>>snapshotAs:thenQuit:, at the
        // send that asks whether the snapshot primitive answered nil. That is
        // how a resumed image tells itself apart from the one that saved it.
        let association = OOP::from_raw(SCHEDULER_ASSOCIATION_POINTER);
        let scheduler = OOP::from_raw(memory.fetch_pointer(VALUE_INDEX, association));
        let process = OOP::from_raw(memory.fetch_pointer(ACTIVE_PROCESS_INDEX, scheduler));
        let context = OOP::from_raw(memory.fetch_pointer(SUSPENDED_CONTEXT_INDEX, process));
        let method = OOP::from_raw(memory.fetch_pointer(METHOD_INDEX, context));

        // The context stores its instruction pointer one relative.
        let resume = (memory.fetch_pointer(INSTRUCTION_POINTER_INDEX, context) >> 1) as usize - 1;

        // Walk the whole method rather than stopping at the resume point, so
        // that the forms this stretch does not reach are covered too.
        let mut walk: Vec<(usize, String)> = Vec::new();
        let mut ip = first_bytecode_of(&memory, method);
        while ip < memory.fetch_byte_length_of(method) as usize {
            let instruction = decode(&memory, method, ip);
            walk.push((ip, instruction.text));
            ip += instruction.length;
        }

        // Landing on the resume point is necessary but not sufficient: a wrong
        // operand width decodes an operand as a bytecode, and a bytecode stream
        // resynchronises readily enough to arrive at the right offset anyway. So
        // stretches of the walk are pinned instruction by instruction.
        let at = |offset: usize| {
            walk.iter()
                .find(|(ip, _)| *ip == offset)
                .unwrap_or_else(|| panic!("nothing decoded at {}, the walk missed it", offset))
                .1
                .as_str()
        };

        assert_eq!(
            walk[..7],
            [
                (76, "push binding #Cursor".to_string()),
                (77, "send #execute (0 args)".to_string()),
                (78, "send #show (0 args)".to_string()),
                (79, "pop".to_string()),
                (80, "push self".to_string()),
                (81, "push literal '\'----SNAPSHOT----\''".to_string()),
                (82, "send #logChange: (1 args)".to_string()),
            ]
        );
        // A special selector, whose argument count comes from the shared array
        // rather than from the bytecode.
        assert_eq!(at(86), "send #== (1 args)");
        // A conditional jump, which takes its displacement from the next byte.
        assert_eq!(at(87), "jump 13 if true");
        // A single extended send, which packs its argument count and literal
        // index into one descriptor byte.
        assert_eq!(at(126), "send #min: (1 args)");
        // The run up to the resume point, contiguous so that a length that is
        // too short shows up as an extra instruction rather than resynchronising
        // and arriving at the same place anyway.
        let resume_index = walk
            .iter()
            .position(|(ip, _)| *ip == resume)
            .expect("the walk never reached the resume point");
        assert_eq!(
            walk[resume_index - 3..=resume_index],
            [
                (139, "pop".to_string()),
                (140, "push self".to_string()),
                (141, "send #snapshotPrimitive (0 args)".to_string()),
                (143, "send #isNil (0 args)".to_string()),
            ]
        );
        // Double extended sends, which spend a byte each on count and literal.
        assert_eq!(at(148), "send #& (1 args)");
        assert_eq!(at(153), "send #quitPrimitive (0 args)");
    }

    #[test]
    fn byte_object_text_reads_a_symbol_from_the_image() {
        let memory = snapshot();

        assert_eq!(
            byte_object_text(&memory, OOP::from_raw(DOES_NOT_UNDERSTAND_SELECTOR)),
            "doesNotUnderstand:",
            "the trailing colon is the odd byte, so the length has to come from \
             fetch_byte_length_of rather than the word length"
        );
    }

    #[test]
    fn class_name_reads_the_symbol_a_class_holds() {
        let memory = snapshot();
        let class = OOP::from_raw(memory.fetch_class_of(OOP::from_raw(NIL_POINTER)));

        assert_eq!(class_name(&memory, class), "UndefinedObject");
    }

    #[test]
    fn class_name_names_a_metaclass_after_the_class_it_belongs_to() {
        let memory = snapshot();
        // String's class is a Metaclass, whose field 6 holds String itself
        // rather than a name of its own.
        let metaclass = OOP::from_raw(memory.fetch_class_of(OOP::from_raw(CLASS_STRING_POINTER)));

        assert_eq!(class_name(&memory, metaclass), "String class");
        // Metaclass is an ordinary class, so the recursion stops one level up.
        assert_eq!(
            class_name(&memory, OOP::from_raw(memory.fetch_class_of(metaclass))),
            "Metaclass"
        );
    }
}

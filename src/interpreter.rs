use crate::{
    globalconstants::{
        HOME_INDEX, INSTRUCTION_POINTER_INDEX, LITERAL_START, METHOD_INDEX, NIL_POINTER,
        RECEIVER_INDEX, STACK_POINTER_INDEX, TEMP_FRAME_START,
    },
    objectmemory::ObjectMemory,
    oop::OOP,
};

pub struct Interpreter {
    memory: ObjectMemory,

    // context cache
    active_context: OOP,
    home_context: OOP, // == active for MethodContext, follows .sender chain for BlockContext
    method: OOP,       // current CompiledMethod
    receiver: OOP,

    ip: usize, // byte offset into method
    sp: usize, // top-of-stack index in active_context

    // send-time scratch
    message_selector: OOP,
    argument_count: u8,
    new_method: OOP,
    primitive_index: u8,

    current_bytecode: u8,
}

impl Interpreter {
    pub fn new(memory: ObjectMemory, active_context: OOP) -> Self {
        let mut interpreter = Interpreter {
            memory,
            active_context,
            home_context: OOP::from_raw(NIL_POINTER),
            method: OOP::from_raw(NIL_POINTER),
            receiver: OOP::from_raw(NIL_POINTER),
            ip: 0,
            sp: 0,
            message_selector: OOP::from_raw(NIL_POINTER),
            argument_count: 0,
            new_method: OOP::from_raw(NIL_POINTER),
            primitive_index: 0,
            current_bytecode: 0,
        };
        interpreter.fetch_context_registers();
        interpreter
    }

    /// Runs the main interpreter loop, repeatedly executing one cycle until
    /// execution terminates.
    //
    // see Bluebook p. 594
    //
    // interpret
    //   [true] whileTrue: [self cycle]
    pub fn interpret(&mut self) {
        loop {
            self.cycle();
        }
    }

    /// Executes a single interpreter step: checks for a pending process switch,
    /// fetches the next bytecode into current_bytecode, and dispatches it.
    //
    // see Bluebook p. 594
    //
    // cycle
    //   self checkProcessSwitch.
    //   currentBytecode ← self fetchByte.
    //   self dispatchOnThisBytecode
    //
    // checkProcessSwitch is omitted until process scheduling exists.
    fn cycle(&mut self) {
        // self.check_process_switch(); // TODO: scheduling, will be done later
        self.current_bytecode = self.fetch_byte();
        self.dispatch_on_this_bytecode();
    }

    /// Dispatches current_bytecode to the routine that implements it.
    //
    // see Bluebook p. 595
    //
    // dispatchOnThisBytecode
    //   (currentBytecode between: 0 and: 119) ifTrue: [↑self stackBytecode].
    //   (currentBytecode between: 120 and: 127) ifTrue: [↑self returnBytecode].
    //   (currentBytecode between: 128 and: 130) ifTrue: [↑self stackBytecode].
    //   (currentBytecode between: 131 and: 134) ifTrue: [↑self sendBytecode].
    //   (currentBytecode between: 135 and: 137) ifTrue: [↑self stackBytecode].
    //   (currentBytecode between: 144 and: 175) ifTrue: [↑self jumpBytecode].
    //   (currentBytecode between: 176 and: 255) ifTrue: [↑self sendBytecode]
    //
    // The Bluebook's nested category dispatch (stack/jump/send/return) is collapsed
    // into a single Rust match, which is the jump table the between:and: chain emulates.
    fn dispatch_on_this_bytecode(&mut self) {
        match self.current_bytecode {
            0..=119 => self.stack_bytecode(),
            120..=127 => self.return_bytecode(),
            128..=130 => self.stack_bytecode(),
            131..=134 => self.send_bytecode(),
            135..=137 => self.stack_bytecode(),
            144..=175 => self.jump_bytecode(),
            176..=255 => self.send_bytecode(),
            _ => panic!("bytecode {} not implemented", self.current_bytecode),
        }
    }

    /// Second-level dispatch for the stack bytecodes: pushes, stores, and the
    /// stack-manipulation operations.
    //
    // see Bluebook p. 597
    //
    // stackBytecode
    //   (currentBytecode between: 0 and: 15) ifTrue: [↑self pushReceiverVariableBytecode].
    //   (currentBytecode between: 16 and: 31) ifTrue: [↑self pushTemporaryVariableBytecode].
    //   (currentBytecode between: 32 and: 63) ifTrue: [↑self pushLiteralConstantBytecode].
    //   (currentBytecode between: 64 and: 95) ifTrue: [↑self pushLiteralVariableBytecode].
    //   (currentBytecode between: 96 and: 103) ifTrue: [↑self storeAndPopReceiverVariableBytecode].
    //   (currentBytecode between: 104 and: 111) ifTrue: [↑self storeAndPopTemporaryVariableBytecode].
    //   currentBytecode = 112 ifTrue: [↑self pushReceiverBytecode].
    //   (currentBytecode between: 113 and: 119) ifTrue: [↑self pushConstantBytecode].
    //   currentBytecode = 128 ifTrue: [↑self extendedPushBytecode].
    //   currentBytecode = 129 ifTrue: [↑self extendedStoreBytecode].
    //   currentBytecode = 130 ifTrue: [↑self extendedStoreAndPopBytecode].
    //   currentBytecode = 135 ifTrue: [↑self popStackBytecode].
    //   currentBytecode = 136 ifTrue: [↑self duplicateTopBytecode].
    //   currentBytecode = 137 ifTrue: [↑self pushActiveContextBytecode]
    fn stack_bytecode(&mut self) {
        match self.current_bytecode {
            0..=15 => self.push_receiver_variable_bytecode(),
            16..=31 => self.push_temporary_variable_bytecode(),
            32..=63 => self.push_literal_constant_bytecode(),
            _ => panic!("bytecode {} not implemented", self.current_bytecode),
        }
    }

    /// Second-level dispatch for the return bytecodes: returns a value to the sender
    /// (method return) or to the caller (block return).
    //
    // see Bluebook p. 608
    //
    // returnBytecode
    //   currentBytecode = 120 ifTrue: [↑self returnValue: receiver to: self sender].
    //   currentBytecode = 121 ifTrue: [↑self returnValue: TruePointer to: self sender].
    //   currentBytecode = 122 ifTrue: [↑self returnValue: FalsePointer to: self sender].
    //   currentBytecode = 123 ifTrue: [↑self returnValue: NilPointer to: self sender].
    //   currentBytecode = 124 ifTrue: [↑self returnValue: self popStack to: self sender].
    //   currentBytecode = 125 ifTrue: [↑self returnValue: self popStack to: self caller]
    fn return_bytecode(&mut self) {
        todo!()
    }

    /// Second-level dispatch for the send bytecodes: extended sends, special-selector
    /// sends, and literal-selector sends.
    //
    // see Bluebook p. 604
    //
    // sendBytecode
    //   (currentBytecode between: 131 and: 134) ifTrue: [↑self extendedSendBytecode].
    //   (currentBytecode between: 176 and: 207) ifTrue: [↑self sendSpecialSelectorBytecode].
    //   (currentBytecode between: 208 and: 255) ifTrue: [↑self sendLiteralSelectorBytecode]
    fn send_bytecode(&mut self) {
        todo!()
    }

    /// Second-level dispatch for the jump bytecodes (conditional and unconditional,
    /// short and long).
    //
    // see Bluebook p. 601
    //
    // jumpBytecode
    //   (currentBytecode between: 144 and: 151) ifTrue: [↑self shortUnconditionalJump].
    //   (currentBytecode between: 152 and: 159) ifTrue: [↑self shortConditionalJump].
    //   (currentBytecode between: 160 and: 167) ifTrue: [↑self longUnconditionalJump].
    //   (currentBytecode between: 168 and: 175) ifTrue: [↑self longConditionalJump]
    fn jump_bytecode(&mut self) {
        todo!()
    }

    /// Loads the interpreter's cached context registers (home_context, receiver,
    /// method, ip, sp) from the active context object on the heap.
    ///
    /// receiver and method are read from the home context (the active context itself
    /// for a MethodContext, or the context's home field for a BlockContext), while ip
    /// and sp are read from the active context.
    //
    // see Bluebook p. 583
    //
    // fetchContextRegisters
    //   (self isBlockContext: activeContext)
    //     ifTrue: [homeContext ← memory fetchPointer: HomeIndex ofObject: activeContext]
    //     ifFalse: [homeContext ← activeContext].
    //   receiver ← memory fetchPointer: ReceiverIndex ofObject: homeContext.
    //   method ← memory fetchPointer: MethodIndex ofObject: homeContext.
    //   instructionPointer ← (self instructionPointerOfContext: activeContext) - 1.
    //   stackPointer ← (self stackPointerOfContext: activeContext) + TempFrameStart - 1
    fn fetch_context_registers(&mut self) {
        self.home_context = if self.is_block_context(self.active_context) {
            OOP::from_raw(self.memory.fetch_pointer(HOME_INDEX, self.active_context))
        } else {
            self.active_context
        };
        self.receiver = OOP::from_raw(self.memory.fetch_pointer(RECEIVER_INDEX, self.home_context));
        self.method = OOP::from_raw(self.memory.fetch_pointer(METHOD_INDEX, self.home_context));

        // pc and sp are SmallIntegers; the stored pc is a 1-relative byte index and the
        // stored sp is relative to the temp frame start, so both are normalized to
        // 0-relative indices here (and converted back in store_context_registers).
        let pc_oop = OOP::from_raw(
            self.memory
                .fetch_pointer(INSTRUCTION_POINTER_INDEX, self.active_context),
        );
        let sp_oop = OOP::from_raw(
            self.memory
                .fetch_pointer(STACK_POINTER_INDEX, self.active_context),
        );
        self.ip = (pc_oop.integer_value_of().expect("pc not a SmallInteger") - 1) as usize;
        self.sp = (sp_oop.integer_value_of().expect("sp not a SmallInteger")
            + TEMP_FRAME_START as i16
            - 1) as usize;
    }

    /// Writes the cached ip and sp registers back into the active context object on
    /// the heap as SmallIntegers.
    ///
    /// receiver, method and home_context are not written back because they do not
    /// change during the execution of a single context.
    //
    // see Bluebook p. 584
    //
    // storeContextRegisters
    //   self storeInstructionPointerValue: instructionPointer + 1 inContext: activeContext.
    //   self storeStackPointerValue: stackPointer - TempFrameStart + 1 inContext: activeContext
    fn store_context_registers(&mut self) {
        let pc_oop =
            OOP::integer_object_of(self.ip as i16 + 1).expect("pc out of SmallInteger range");
        let sp_oop = OOP::integer_object_of(self.sp as i16 - TEMP_FRAME_START as i16 + 1)
            .expect("sp out of SmallInteger range");
        self.memory
            .store_pointer(INSTRUCTION_POINTER_INDEX, self.active_context, pc_oop.value);
        self.memory
            .store_pointer(STACK_POINTER_INDEX, self.active_context, sp_oop.value);
    }

    /// Determines whether a context is a BlockContext or a MethodContext.
    ///
    /// # Parameters:
    ///     * context (OOP): The context object to inspect
    ///
    /// # Returns:
    ///     * bool, true if the context is a BlockContext, false if it is a MethodContext
    //
    // see Bluebook p. 584
    //
    // isBlockContext: contextPointer
    //   | methodOrArguments |
    //   methodOrArguments ← memory fetchPointer: MethodIndex ofObject: contextPointer.
    //   ↑memory isIntegerObject: methodOrArguments
    //
    fn is_block_context(&self, context: OOP) -> bool {
        // Both types of contexts use the fourth field (index 3) for different information
        // MethodContexts use it to store an object pointer (the CompiledMethod)
        // BlockContexts use it to store the argument count which is stored as a SmallInteger
        // Hence we can determine the type of context by checking if the OOP is an integer object
        // (which is a 1 bit check)
        OOP::from_raw(self.memory.fetch_pointer(METHOD_INDEX, context)).is_integer_object()
    }

    /// Fetches the byte at the current instruction pointer from the method and
    /// advances the instruction pointer past it.
    ///
    /// # Returns:
    ///     * u8, the fetched bytecode
    //
    // see Bluebook p. 594
    //
    // fetchByte
    //   | byte |
    //   byte ← memory fetchByte: instructionPointer ofObject: method.
    //   instructionPointer ← instructionPointer + 1.
    //   ↑byte
    fn fetch_byte(&mut self) -> u8 {
        let byte = self.memory.fetch_byte(self.ip as u16, self.method);
        self.ip += 1;
        byte
    }

    /// Extracts a right-aligned bit field from a 16-bit value, using the Bluebook's
    /// bit numbering where bit 0 is the high-order (most significant) bit and bit 15
    /// is the low-order (least significant) bit.
    ///
    /// # Parameters:
    ///     * first_bit_index (u8): The index of the first (most significant) bit of the field
    ///     * last_bit_index (u8): The index of the last (least significant) bit of the field
    ///     * value (u16): The value to extract the bits from
    ///
    /// # Returns:
    ///     * u16, the extracted bit field, shifted down to bit 15
    //
    // see Bluebook p. 575
    //
    // extractBits: firstBitIndex to: lastBitIndex of: anInteger
    //   ↑(anInteger bitShift: lastBitIndex - 15)
    //     bitAnd: (2 raisedTo: lastBitIndex - firstBitIndex + 1) - 1
    fn extract_bits(first_bit_index: u8, last_bit_index: u8, value: u16) -> u16 {
        let ret = value << first_bit_index;
        ret >> (first_bit_index + (15 - last_bit_index))
    }

    /// Pushes an object pointer onto the active context's evaluation stack.
    ///
    /// # Parameters:
    ///     * value (OOP): The object pointer to push
    //
    // see Bluebook p. 585
    //
    // push: object
    //   stackPointer ← stackPointer + 1.
    //   memory storePointer: stackPointer ofObject: activeContext withValue: object
    fn push(&mut self, value: OOP) {
        self.sp += 1;
        self.memory
            .store_pointer(self.sp as u16, self.active_context, value.value);
    }

    /// Removes and returns the top object pointer of the active context's evaluation
    /// stack.
    ///
    /// # Returns:
    ///     * OOP, the object pointer that was on top of the stack
    //
    // see Bluebook p. 585
    //
    // popStack
    //   | stackTop |
    //   stackTop ← memory fetchPointer: stackPointer ofObject: activeContext.
    //   stackPointer ← stackPointer - 1.
    //   ↑stackTop
    fn pop_stack(&mut self) -> OOP {
        let stack_top = self
            .memory
            .fetch_pointer(self.sp as u16, self.active_context);
        self.sp -= 1;
        OOP::from_raw(stack_top)
    }

    /// Returns the top object pointer of the active context's evaluation stack
    /// without removing it.
    ///
    /// # Returns:
    ///     * OOP, the object pointer on top of the stack
    //
    // see Bluebook p. 585
    //
    // stackTop
    //   ↑memory fetchPointer: stackPointer ofObject: activeContext
    fn stack_top(&self) -> OOP {
        OOP::from_raw(
            self.memory
                .fetch_pointer(self.sp as u16, self.active_context),
        )
    }

    /// Returns the object pointer at the given offset below the top of the active
    /// context's evaluation stack, without removing it.
    ///
    /// # Parameters:
    ///     * offset (usize): How far below the top of the stack to read (0 is the top)
    ///
    /// # Returns:
    ///     * OOP, the object pointer at that stack position
    //
    // see Bluebook p. 585
    //
    // stackValue: offset
    //   ↑memory fetchPointer: stackPointer-offset ofObject: activeContext
    fn stack_value(&self, offset: usize) -> OOP {
        OOP::from_raw(
            self.memory
                .fetch_pointer((self.sp - offset) as u16, self.active_context),
        )
    }

    /// Discards the given number of object pointers from the top of the active
    /// context's evaluation stack.
    ///
    /// # Parameters:
    ///     * number (usize): How many entries to drop from the stack
    //
    // see Bluebook p. 585
    //
    // pop: number
    //   stackPointer ← stackPointer - number
    fn pop(&mut self, number: usize) {
        self.sp -= number
    }

    /// Restores the given number of object pointers onto the active context's
    /// evaluation stack by moving the stack pointer back up.
    ///
    /// # Parameters:
    ///     * number (usize): How many entries to restore to the stack
    //
    // see Bluebook p. 585
    //
    // unPop: number
    //   stackPointer ← stackPointer + number
    fn un_pop(&mut self, number: usize) {
        self.sp += number
    }

    /// Returns the home context's temporary frame location at the given offset.
    ///
    /// # Parameters:
    ///     * offset (u16): The index into the temporary frame (arguments then temporaries)
    ///
    /// # Returns:
    ///     * OOP, the object pointer stored at that temporary frame location
    //
    // see Bluebook p. 586
    //
    // temporary: offset
    //   ↑memory fetchPointer: offset + TempFrameStart ofObject: homeContext
    fn temporary(&self, offset: u16) -> OOP {
        OOP::from_raw(
            self.memory
                .fetch_pointer(offset + TEMP_FRAME_START, self.home_context),
        )
    }

    /// Returns the currently executing method's literal at the given offset.
    ///
    /// # Parameters:
    ///     * offset (u16): The index into the method's literal frame
    ///
    /// # Returns:
    ///     * OOP, the object pointer stored at that literal frame location
    //
    // see Bluebook p. 586
    //
    // literal: offset
    //   ↑self literal: offset ofMethod: method
    fn literal(&self, offset: u16) -> OOP {
        self.literal_of_method(offset, self.method)
    }

    /// Returns the given method's literal at the given offset. Shared with the method
    /// header utilities, which read literals of methods other than the active one.
    ///
    /// # Parameters:
    ///     * offset (u16): The index into the method's literal frame
    ///     * method_pointer (OOP): The CompiledMethod to read the literal from
    ///
    /// # Returns:
    ///     * OOP, the object pointer stored at that literal frame location
    //
    // see Bluebook p. 577
    //
    // literal: offset ofMethod: methodPointer
    //   ↑memory fetchPointer: offset + LiteralStart ofObject: methodPointer
    fn literal_of_method(&self, offset: u16, method_pointer: OOP) -> OOP {
        OOP::from_raw(
            self.memory
                .fetch_pointer(offset + LITERAL_START, method_pointer),
        )
    }

    /// Extracts the receiver-variable field index from the current bytecode and
    /// pushes that instance variable onto the stack.
    //
    // see Bluebook p. 598
    //
    // pushReceiverVariableBytecode
    //   | fieldIndex |
    //   fieldIndex ← self extractBits: 12 to: 15 of: currentBytecode.
    //   self pushReceiverVariable: fieldIndex
    fn push_receiver_variable_bytecode(&mut self) {
        let field_index = Self::extract_bits(12, 15, self.current_bytecode as u16);
        self.push_receiver_variable(field_index);
    }

    /// Pushes the receiver's instance variable at the given field index onto the
    /// stack. Shared by the short bytecode form and the extended push bytecode.
    ///
    /// # Parameters:
    ///     * field_index (u16): The index of the receiver's instance variable to push
    //
    // see Bluebook p. 598
    //
    // pushReceiverVariable: fieldIndex
    //   self push: (memory fetchPointer: fieldIndex ofObject: receiver)
    fn push_receiver_variable(&mut self, field_index: u16) {
        self.push(OOP::from_raw(
            self.memory.fetch_pointer(field_index, self.receiver),
        ))
    }

    /// Extracts the temporary location index from the current bytecode and pushes
    /// that temporary frame location onto the stack.
    //
    // see Bluebook p. 598
    //
    // pushTemporaryVariableBytecode
    //   | fieldIndex |
    //   fieldIndex ← self extractBits: 12 to: 15 of: currentBytecode.
    //   self pushTemporaryVariable: fieldIndex
    fn push_temporary_variable_bytecode(&mut self) {
        let temporary_index = Self::extract_bits(12, 15, self.current_bytecode as u16);
        self.push_temporary_variable(temporary_index);
    }

    /// Pushes the active context's temporary frame location at the given index onto
    /// the stack. Shared by the short bytecode form and the extended push bytecode.
    ///
    /// # Parameters:
    ///     * temporary_index (u16): The index of the temporary frame location to push
    //
    // see Bluebook p. 598
    //
    // pushTemporaryVariable: temporaryIndex
    //   self push: (self temporary: temporaryIndex)
    fn push_temporary_variable(&mut self, temporary_index: u16) {
        self.push(self.temporary(temporary_index));
    }

    /// Extracts the literal index from the current bytecode and pushes that literal
    /// constant onto the stack.
    //
    // see Bluebook p. 598
    //
    // pushLiteralConstantBytecode
    //   | fieldIndex |
    //   fieldIndex ← self extractBits: 11 to: 15 of: currentBytecode.
    //   self pushLiteralConstant: fieldIndex
    fn push_literal_constant_bytecode(&mut self) {
        let literal_index = Self::extract_bits(11, 15, self.current_bytecode as u16);
        self.push_literal_constant(literal_index);
    }

    /// Pushes the method's literal constant at the given index onto the stack.
    /// Shared by the short bytecode form and the extended push bytecode.
    ///
    /// # Parameters:
    ///     * literal_index (u16): The index of the literal constant to push
    //
    // see Bluebook p. 598
    //
    // pushLiteralConstant: literalIndex
    //   self push: (self literal: literalIndex)
    fn push_literal_constant(&mut self, literal_index: u16) {
        self.push(self.literal(literal_index));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objectmemory::ObjectMemory;
    use crate::oop::OOP;

    #[test]
    fn extract_bits_extracts_correct_field() {
        // bits 12-15: the low nibble
        assert_eq!(Interpreter::extract_bits(12, 15, 5), 5);
        // bits 11-15: the low 5 bits
        assert_eq!(Interpreter::extract_bits(11, 15, 0x1F), 0x1F);
        // bits 8-9: the top 2 bits of a byte-sized value
        assert_eq!(Interpreter::extract_bits(8, 9, 0xC0), 3);
        // bit 0: the single high-order bit
        assert_eq!(Interpreter::extract_bits(0, 0, 0x8000), 1);
    }

    // Tag a value as a SmallInteger object pointer (low bit set).
    fn small_int(value: u16) -> u16 {
        (value << 1) | 1
    }

    #[test]
    fn push_receiver_variable_bytecode_executes_end_to_end() {
        // A hand-built minimal image with three objects, all in segment 0:
        //   OOP 4 = receiver       (one SmallInteger instance variable, 42)
        //   OOP 6 = active context (a MethodContext)
        //   OOP 8 = method         (a CompiledMethod whose first bytecode is 0x00,
        //                           "push receiver variable 0")
        //
        // Object table: entry for OOP n lives at words [n] = count<<8 | segment and
        // [n+1] = heap location.
        let mut ot = vec![0u16; 16];
        ot[4] = 0x0100; // receiver: count 1, segment 0
        ot[5] = 20; //          location 20
        ot[6] = 0x0100; // context
        ot[7] = 30;
        ot[8] = 0x0100; // method
        ot[9] = 40;

        // Heap segment 0. Each object is [size, class, ...body]; the 2-word header
        // means body field f lives at location + 2 + f.
        let mut seg = vec![0u16; 64];

        // receiver @ 20: instance variable 0 = SmallInteger 42
        seg[20] = 3; // size: 2 header + 1 field
        seg[21] = 0; // class (not read on this path)
        seg[22] = small_int(42); // instance variable 0

        // method @ 40: header then bytecodes. Body word 1 (bytes 2..3) is the first
        // bytecode word; byte 2 = 0x00 = push receiver variable 0.
        seg[40] = 4; // size: 2 header + 2 body words
        seg[41] = 0; // class
        seg[42] = small_int(0); // method header (0 literals; not decoded here)
        seg[43] = 0x0000; // bytecodes: byte 2 = 0x00

        // context @ 30 (MethodContext). Body fields:
        //   0 sender, 1 ip, 2 sp, 3 method, 4 unused, 5 receiver, 6 stack base
        seg[30] = 9; // size: 2 header + 7 fields
        seg[31] = 0; // class
        seg[32] = 2; // sender = nil (not read)
        seg[33] = small_int(3); // ip: stored 1-relative = (literalCount+1)*2+1 = 3
        seg[34] = small_int(0); // sp: stored = 0 (empty stack, no temps)
        seg[35] = 8; // method = OOP 8 (object pointer => MethodContext)
        seg[36] = 2; // unused
        seg[37] = 4; // receiver = OOP 4
        seg[38] = small_int(0); // first stack slot, pre-filled SmallInteger

        let memory = ObjectMemory::from_image(vec![seg], ot);
        let mut interpreter = Interpreter::new(memory, OOP::from_raw(6));

        // new() ran fetch_context_registers: registers reflect the context.
        assert_eq!(interpreter.receiver, OOP::from_raw(4));
        assert_eq!(interpreter.ip, 2); // stored 3 normalized to 0-relative byte 2
        assert_eq!(interpreter.sp, 5); // stored 0 + TempFrameStart - 1

        // Execute one bytecode end to end: fetch_byte -> dispatch -> stack_bytecode
        // -> push_receiver_variable_bytecode -> push_receiver_variable -> push.
        interpreter.cycle();

        assert_eq!(interpreter.ip, 3); // advanced past the one bytecode
        assert_eq!(interpreter.sp, 6); // one value pushed
        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(42));
    }
}

use crate::{
    globalconstants::{
        FALSE_POINTER, HOME_INDEX, INSTRUCTION_POINTER_INDEX, LITERAL_START, METHOD_INDEX,
        MINUS_ONE_POINTER, NIL_POINTER, ONE_POINTER, RECEIVER_INDEX, STACK_POINTER_INDEX,
        TEMP_FRAME_START, TRUE_POINTER, TWO_POINTER, VALUE_INDEX, ZERO_POINTER,
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

    /// Aborts the interpreter, reporting where it stopped.
    ///
    /// For conditions that mean the image is malformed or the interpreter has
    /// walked off its own rails, where no Smalltalk-level recovery exists. The
    /// failures Smalltalk itself handles (doesNotUnderstand, mustBeBoolean,
    /// cannotReturn, and primitive failure) are ordinary control flow and must
    /// not come through here. Conditions that cannot occur at all belong in an
    /// unreachable! instead, so that a firing panic always means bad input
    /// rather than a bug in the dispatch above it.
    ///
    /// The registers reported are whatever is currently cached, so a call from
    /// fetch_context_registers still describes the context being left.
    ///
    /// # Parameters:
    ///     * message (&str): What went wrong
    fn vm_panic(&self, message: &str) -> ! {
        panic!(
            "{message} (bytecode {:#04x}, ip {}, method {:#06x}, receiver {:#06x}, context {:#06x})",
            self.current_bytecode,
            self.ip,
            self.method.value,
            self.receiver.value,
            self.active_context.value
        )
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
            // 138 through 143 are the gaps in the bytecode set; the compiler
            // never emits them, so reaching one means a malformed method.
            138..=143 => self.vm_panic(&format!("unused bytecode {}", self.current_bytecode)),
            144..=175 => self.jump_bytecode(),
            176..=255 => self.send_bytecode(),
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
            64..=95 => self.push_literal_variable_bytecode(),
            96..=103 => self.store_and_pop_receiver_variable_bytecode(),
            104..=111 => self.store_and_pop_temporary_variable_bytecode(),
            112 => self.push_receiver_bytecode(),
            113..=119 => self.push_constant_bytecode(),
            128 => self.extended_push_bytecode(),
            129 => self.extended_store_bytecode(),
            130 => self.extended_store_and_pop_bytecode(),
            135 => self.pop_stack_bytecode(),
            136 => self.duplicate_top_bytecode(),
            137 => self.push_active_context_bytecode(),
            _ => unreachable!("{} is not a stack bytecode", self.current_bytecode),
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
        let pc = pc_oop
            .integer_value_of()
            .unwrap_or_else(|_| self.vm_panic("context pc is not a SmallInteger"));
        let sp = sp_oop
            .integer_value_of()
            .unwrap_or_else(|_| self.vm_panic("context sp is not a SmallInteger"));
        self.ip = (pc - 1) as usize;
        self.sp = (sp + TEMP_FRAME_START as i16 - 1) as usize;
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
        let pc_oop = OOP::integer_object_of(self.ip as i16 + 1)
            .unwrap_or_else(|_| self.vm_panic("pc out of SmallInteger range"));
        let sp_oop = OOP::integer_object_of(self.sp as i16 - TEMP_FRAME_START as i16 + 1)
            .unwrap_or_else(|_| self.vm_panic("sp out of SmallInteger range"));
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

    /// Extracts the literal index from the current bytecode and pushes the value
    /// field of the Association stored in that literal onto the stack.
    //
    // see Bluebook p. 598
    //
    // pushLiteralVariableBytecode
    //   | fieldIndex |
    //   fieldIndex ← self extractBits: 11 to: 15 of: currentBytecode.
    //   self pushLiteralVariable: fieldIndex
    fn push_literal_variable_bytecode(&mut self) {
        let field_index = Self::extract_bits(11, 15, self.current_bytecode as u16);
        self.push_literal_variable(field_index);
    }

    /// Pushes the value field of the Association stored in the method's literal
    /// frame at the given index onto the stack. Shared by the short bytecode form
    /// and the extended push bytecode.
    ///
    /// Literal variables are the shared variables (globals, class variables and pool
    /// variables); the literal holds an Association binding a name to a value, and it
    /// is the value that is pushed, not the Association itself.
    ///
    /// # Parameters:
    ///     * literal_index (u16): The index of the Association in the literal frame
    //
    // see Bluebook p. 599
    //
    // pushLiteralVariable: literalIndex
    //   | association |
    //   association ← self literal: literalIndex.
    //   self push: (memory fetchPointer: ValueIndex ofObject: association)
    fn push_literal_variable(&mut self, literal_index: u16) {
        let association = self.literal(literal_index);
        self.push(OOP::from_raw(
            self.memory.fetch_pointer(VALUE_INDEX, association),
        ));
    }

    /// Fetches the descriptor byte following the bytecode and performs the push it
    /// describes: receiver variable, temporary frame location, literal constant or
    /// literal variable.
    ///
    /// The descriptor's high two bits select which of the four pushes to perform and
    /// its low six bits give the index, so this reaches 64 variables where the single
    /// byte forms are limited to 16 or 32.
    //
    // see Bluebook p. 599
    //
    // extendedPushBytecode
    //   | descriptor variableType variableIndex |
    //   descriptor ← self fetchByte.
    //   variableType ← self extractBits: 8 to: 9 of: descriptor.
    //   variableIndex ← self extractBits: 10 to: 15 of: descriptor.
    //   variableType=0 ifTrue: [↑self pushReceiverVariable: variableIndex].
    //   variableType=1 ifTrue: [↑self pushTemporaryVariable: variableIndex].
    //   variableType=2 ifTrue: [↑self pushLiteralConstant: variableIndex].
    //   variableType=3 ifTrue: [↑self pushLiteralVariable: variableIndex]
    fn extended_push_bytecode(&mut self) {
        let descriptor = self.fetch_byte();
        let variable_type = Self::extract_bits(8, 9, descriptor as u16);
        let variable_index = Self::extract_bits(10, 15, descriptor as u16);
        match variable_type {
            0 => self.push_receiver_variable(variable_index),
            1 => self.push_temporary_variable(variable_index),
            2 => self.push_literal_constant(variable_index),
            3 => self.push_literal_variable(variable_index),
            _ => unreachable!("Unexpected variable type {}, expected 0..3", variable_type),
        }
    }

    /// Pushes the active context's receiver onto the stack. This is the bytecode
    /// compiled for self and super.
    //
    // see Bluebook p. 599
    //
    // pushReceiverBytecode
    //   self push: receiver
    fn push_receiver_bytecode(&mut self) {
        self.push(self.receiver);
    }

    /// Pushes another copy of the object pointer on top of the stack.
    //
    // see Bluebook p. 599
    //
    // duplicateTopBytecode
    //   ↑self push: self stackTop
    fn duplicate_top_bytecode(&mut self) {
        self.push(self.stack_top());
    }

    /// Pushes one of the seven constant object pointers (true, false, nil, -1, 0, 1
    /// or 2) selected by the current bytecode.
    //
    // see Bluebook p. 600
    //
    // pushConstantBytecode
    //   currentBytecode = 113 ifTrue: [↑self push: TruePointer].
    //   currentBytecode = 114 ifTrue: [↑self push: FalsePointer].
    //   currentBytecode = 115 ifTrue: [↑self push: NilPointer].
    //   currentBytecode = 116 ifTrue: [↑self push: MinusOnePointer].
    //   currentBytecode = 117 ifTrue: [↑self push: ZeroPointer].
    //   currentBytecode = 118 ifTrue: [↑self push: OnePointer].
    //   currentBytecode = 119 ifTrue: [↑self push: TwoPointer]
    fn push_constant_bytecode(&mut self) {
        match self.current_bytecode {
            113 => self.push(OOP::from_raw(TRUE_POINTER)),
            114 => self.push(OOP::from_raw(FALSE_POINTER)),
            115 => self.push(OOP::from_raw(NIL_POINTER)),
            116 => self.push(OOP::from_raw(MINUS_ONE_POINTER)),
            117 => self.push(OOP::from_raw(ZERO_POINTER)),
            118 => self.push(OOP::from_raw(ONE_POINTER)),
            119 => self.push(OOP::from_raw(TWO_POINTER)),
            _ => unreachable!(
                "How did we get here? push_constant_bytecode with invalid bytecode {}",
                self.current_bytecode
            ),
        }
    }

    /// Pushes a pointer to the active context itself onto its own stack. This is the
    /// bytecode compiled for thisContext.
    //
    // see Bluebook p. 600
    //
    // pushActiveContextBytecode
    //   self push: activeContext
    fn push_active_context_bytecode(&mut self) {
        self.push(self.active_context);
    }

    /// Extracts the receiver-variable index from the current bytecode and pops the
    /// top of the stack into that instance variable.
    //
    // see Bluebook p. 600
    //
    // storeAndPopReceiverVariableBytecode
    //   | variableIndex |
    //   variableIndex ← self extractBits: 13 to: 15 of: currentBytecode.
    //   memory storePointer: variableIndex
    //          ofObject: receiver
    //          withValue: self popStack
    fn store_and_pop_receiver_variable_bytecode(&mut self) {
        let variable_index = Self::extract_bits(13, 15, self.current_bytecode as u16);
        let value = self.pop_stack().value;
        self.memory
            .store_pointer(variable_index, self.receiver, value);
    }

    /// Extracts the temporary location index from the current bytecode and pops the
    /// top of the stack into that temporary frame location.
    ///
    /// The store targets home_context rather than active_context: the temporary frame
    /// belongs to the home method, so a block assigning to one of its enclosing
    /// method's temporaries writes through to the method's frame.
    //
    // see Bluebook p. 600
    //
    // storeAndPopTemporaryVariableBytecode
    //   | variableIndex |
    //   variableIndex ← self extractBits: 13 to: 15 of: currentBytecode.
    //   memory storePointer: variableIndex + TempFrameStart
    //          ofObject: homeContext
    //          withValue: self popStack
    fn store_and_pop_temporary_variable_bytecode(&mut self) {
        let variable_index = Self::extract_bits(13, 15, self.current_bytecode as u16);
        let value = self.pop_stack().value;
        self.memory
            .store_pointer(variable_index + TEMP_FRAME_START, self.home_context, value);
    }

    /// Performs the store described by the following descriptor byte and then pops
    /// the stored value off the stack.
    //
    // see Bluebook p. 600
    //
    // extendedStoreAndPopBytecode
    //   self extendedStoreBytecode.
    //   self popStackBytecode
    fn extended_store_and_pop_bytecode(&mut self) {
        self.extended_store_bytecode();
        self.pop_stack_bytecode();
    }

    /// Fetches the descriptor byte following the bytecode and stores the top of the
    /// stack into the location it describes, leaving the value on the stack.
    ///
    /// The descriptor has the same form as the extended push, but a variable type of
    /// 2 (literal constant) is illegal because it would mean changing the value of a
    /// constant, and the compiler never emits it.
    //
    // see Bluebook pp. 600-601
    //
    // extendedStoreBytecode
    //   | descriptor variableType variableIndex association |
    //   descriptor ← self fetchByte.
    //   variableType ← self extractBits: 8 to: 9 of: descriptor.
    //   variableIndex ← self extractBits: 10 to: 15 of: descriptor.
    //   variableType=0 ifTrue:
    //     [↑memory storePointer: variableIndex
    //              ofObject: receiver
    //              withValue: self stackTop].
    //   variableType=1 ifTrue:
    //     [↑memory storePointer: variableIndex + TempFrameStart
    //              ofObject: homeContext
    //              withValue: self stackTop].
    //   variableType=2 ifTrue: [↑self error: 'illegal store'].
    //   variableType=3 ifTrue:
    //     [association ← self literal: variableIndex.
    //      ↑memory storePointer: ValueIndex
    //               ofObject: association
    //               withValue: self stackTop]
    fn extended_store_bytecode(&mut self) {
        let descriptor = self.fetch_byte();
        let variable_type = Self::extract_bits(8, 9, descriptor as u16);
        let variable_index = Self::extract_bits(10, 15, descriptor as u16);
        let value = self.stack_top().value;
        match variable_type {
            0 => {
                self.memory
                    .store_pointer(variable_index, self.receiver, value);
            }
            1 => {
                self.memory.store_pointer(
                    variable_index + TEMP_FRAME_START,
                    self.home_context,
                    value,
                );
            }
            // Storing into a literal constant. The compiler never emits this,
            // so a method containing it is malformed.
            2 => self.vm_panic("illegal store into a literal constant"),
            3 => {
                let association = self.literal(variable_index);
                self.memory.store_pointer(VALUE_INDEX, association, value);
            }
            _ => unreachable!("Unexpected variable type {}, expected 0..3", variable_type),
        }
    }

    /// Removes the top object pointer from the stack without doing anything else
    /// with it.
    //
    // see Bluebook p. 601
    //
    // popStackBytecode
    //   self popStack
    fn pop_stack_bytecode(&mut self) {
        let _ = self.pop_stack();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::globalconstants::{HEADER_SIZE, SENDER_INDEX};
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

    // ┌─────────────────────┐
    // │       FIXTURE       │
    // └─────────────────────┘

    // A hand-built image for exercising one bytecode at a time, so that the
    // per-bytecode tests below say what they are testing instead of restating
    // the heap layout. Everything lives in segment 0.
    //
    // The guaranteed pointers keep their real OOPs, so pushConstantBytecode can
    // be checked against NIL_POINTER and friends without those values colliding
    // with the fixture's own objects.
    //
    //   OOP  2  nil,  OOP  4  false,  OOP  6  true
    //   OOP 16  receiver     RECEIVER_FIELDS instance variables
    //   OOP 18  context      MethodContext: fixed fields, temp frame, then stack
    //   OOP 20  method       CompiledMethod: header, literal frame, bytecodes
    //   OOP 22  association  key at field 0, value at field 1

    const RECEIVER_OOP: u16 = 16;
    const CONTEXT_OOP: u16 = 18;
    const METHOD_OOP: u16 = 20;
    const ASSOCIATION_OOP: u16 = 22;

    // Sized past what the single byte forms can address, so the extended
    // bytecodes have somewhere to reach that the short ones cannot: the short
    // pushes stop at receiver variable and temporary 15, and at literal 31.
    const RECEIVER_FIELDS: u16 = 20;
    const TEMP_COUNT: u16 = 20;
    const STACK_SLOTS: u16 = 8;
    const LITERAL_COUNT: u16 = 40;
    const BYTECODE_WORDS: u16 = 6;

    /// How many variables the short store bytecodes reach: 96..103 for the
    /// receiver and 104..111 for the temp frame. A property of the bytecode set,
    /// not of the fixture's size.
    const SHORT_STORE_COUNT: u8 = 8;

    const NIL_LOCATION: u16 = 10;
    const FALSE_LOCATION: u16 = 14;
    const TRUE_LOCATION: u16 = 18;
    const RECEIVER_LOCATION: u16 = 100;
    const CONTEXT_LOCATION: u16 = 200;
    const METHOD_LOCATION: u16 = 300;
    const ASSOCIATION_LOCATION: u16 = 400;

    /// Byte offset of the first bytecode: the header plus the literal frame, at
    /// two bytes per word. Also the interpreter's initial ip.
    const FIRST_BYTECODE_BYTE: u16 = 2 * (LITERAL_COUNT + 1);

    /// Field index of the first stack slot: the fixed fields plus the temp frame.
    const FIRST_STACK_FIELD: u16 = TEMP_FRAME_START + TEMP_COUNT;

    /// The interpreter's sp when the evaluation stack is empty: one below the
    /// first stack slot, because push pre-increments.
    const EMPTY_STACK_SP: usize = (FIRST_STACK_FIELD - 1) as usize;

    /// An object table entry for a live pointer object in segment 0.
    ///
    /// Word 0 is: bits 15..8 reference count, bit 7 odd, bit 6 pointer, bit 5
    /// free, bits 3..0 segment. The count is set comfortably high so that no
    /// test can drive a fixture object to deallocation through store_pointer's
    /// reference counting.
    const LIVE_POINTER_ENTRY: u16 = (32 << 8) | 0x40;

    struct Fixture {
        ot: Vec<u16>,
        seg: Vec<u16>,
    }

    impl Fixture {
        /// Registers an object in the object table and lays down its header.
        ///
        /// Every field is pre-filled with a SmallInteger: store_pointer decrements
        /// the reference count of whatever it overwrites, and the counter ignores
        /// integers, so a store into any field is guaranteed not to deallocate
        /// something the fixture still needs.
        fn place(ot: &mut [u16], seg: &mut [u16], oop: u16, location: u16, fields: u16) {
            ot[oop as usize] = LIVE_POINTER_ENTRY;
            ot[oop as usize + 1] = location;
            seg[location as usize] = HEADER_SIZE + fields;
            seg[location as usize + 1] = 0; // class: not read on any of these paths
            for field in 0..fields {
                seg[(location + HEADER_SIZE + field) as usize] = small_int(0);
            }
        }

        fn new() -> Self {
            let mut ot = vec![0u16; 32];
            let mut seg = vec![0u16; 512];

            Self::place(&mut ot, &mut seg, NIL_POINTER, NIL_LOCATION, 2);
            Self::place(&mut ot, &mut seg, FALSE_POINTER, FALSE_LOCATION, 2);
            Self::place(&mut ot, &mut seg, TRUE_POINTER, TRUE_LOCATION, 2);
            Self::place(
                &mut ot,
                &mut seg,
                RECEIVER_OOP,
                RECEIVER_LOCATION,
                RECEIVER_FIELDS,
            );
            Self::place(
                &mut ot,
                &mut seg,
                CONTEXT_OOP,
                CONTEXT_LOCATION,
                FIRST_STACK_FIELD + STACK_SLOTS,
            );
            Self::place(
                &mut ot,
                &mut seg,
                METHOD_OOP,
                METHOD_LOCATION,
                1 + LITERAL_COUNT + BYTECODE_WORDS,
            );
            Self::place(&mut ot, &mut seg, ASSOCIATION_OOP, ASSOCIATION_LOCATION, 2);

            // The active MethodContext. ip and sp are stored the way the image
            // stores them: ip 1-relative, sp relative to the temp frame start.
            // With the stack empty, the stored sp is exactly TEMP_COUNT.
            let context = (CONTEXT_LOCATION + HEADER_SIZE) as usize;
            seg[context + SENDER_INDEX as usize] = NIL_POINTER;
            seg[context + INSTRUCTION_POINTER_INDEX as usize] = small_int(FIRST_BYTECODE_BYTE + 1);
            seg[context + STACK_POINTER_INDEX as usize] = small_int(TEMP_COUNT);
            seg[context + METHOD_INDEX as usize] = METHOD_OOP;
            seg[context + RECEIVER_INDEX as usize] = RECEIVER_OOP;

            // The method header. Nothing decodes it yet, but the literal count
            // occupies bits 1..6 of the SmallInteger, so this is the real header
            // for a method with LITERAL_COUNT literals, no temporaries, flag 0.
            let method = (METHOD_LOCATION + HEADER_SIZE) as usize;
            seg[method] = small_int(LITERAL_COUNT);
            for word in 0..BYTECODE_WORDS {
                seg[method + (1 + LITERAL_COUNT + word) as usize] = 0;
            }

            Fixture { ot, seg }
        }

        /// Writes the method's bytecodes, starting at the first bytecode byte.
        fn bytecodes(&mut self, bytes: &[u8]) -> &mut Self {
            for (offset, &byte) in bytes.iter().enumerate() {
                // heap_chunk_of_byte indexes from the start of the chunk, so the
                // two header words count as the first four bytes.
                let raw = (HEADER_SIZE * 2 + FIRST_BYTECODE_BYTE) as usize + offset;
                let word = METHOD_LOCATION as usize + raw / 2;
                self.seg[word] = if raw % 2 == 0 {
                    (self.seg[word] & 0x00FF) | ((byte as u16) << 8)
                } else {
                    (self.seg[word] & 0xFF00) | byte as u16
                };
            }
            self
        }

        fn literal(&mut self, index: u16, value: u16) -> &mut Self {
            self.seg[(METHOD_LOCATION + HEADER_SIZE + LITERAL_START + index) as usize] = value;
            self
        }

        fn instance_variable(&mut self, index: u16, value: u16) -> &mut Self {
            self.seg[(RECEIVER_LOCATION + HEADER_SIZE + index) as usize] = value;
            self
        }

        fn temporary(&mut self, index: u16, value: u16) -> &mut Self {
            self.seg[(CONTEXT_LOCATION + HEADER_SIZE + TEMP_FRAME_START + index) as usize] = value;
            self
        }

        fn association_value(&mut self, value: u16) -> &mut Self {
            self.seg[(ASSOCIATION_LOCATION + HEADER_SIZE + VALUE_INDEX) as usize] = value;
            self
        }

        fn build(&self) -> Interpreter {
            let memory = ObjectMemory::from_image(vec![self.seg.clone()], self.ot.clone());
            Interpreter::new(memory, OOP::from_raw(CONTEXT_OOP))
        }
    }

    /// Reads a field of a fixture object straight out of memory, bypassing the
    /// interpreter, so a test can check where a store actually landed.
    fn field_of(interpreter: &Interpreter, oop: u16, field: u16) -> OOP {
        OOP::from_raw(interpreter.memory.fetch_pointer(field, OOP::from_raw(oop)))
    }

    #[test]
    fn fixture_starts_with_the_expected_registers() {
        let interpreter = Fixture::new().build();

        assert_eq!(interpreter.receiver, OOP::from_raw(RECEIVER_OOP));
        assert_eq!(interpreter.method, OOP::from_raw(METHOD_OOP));
        // A MethodContext is its own home.
        assert_eq!(interpreter.home_context, OOP::from_raw(CONTEXT_OOP));
        assert_eq!(interpreter.ip, FIRST_BYTECODE_BYTE as usize);
        assert_eq!(interpreter.sp, EMPTY_STACK_SP);
    }

    // ┌─────────────────────┐
    // │        PUSHES       │
    // └─────────────────────┘

    #[test]
    fn push_temporary_variable_bytecode_pushes_the_temporary() {
        // 16 = push temporary 0, 19 = push temporary 3
        let mut interpreter = Fixture::new()
            .temporary(0, small_int(11))
            .temporary(3, small_int(33))
            .bytecodes(&[16, 19])
            .build();

        interpreter.cycle();
        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(11));
        interpreter.cycle();
        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(33));
        assert_eq!(interpreter.sp, EMPTY_STACK_SP + 2);
    }

    #[test]
    fn push_literal_constant_bytecode_pushes_the_literal() {
        // 32 = push literal constant 0, 34 = push literal constant 2
        let mut interpreter = Fixture::new()
            .literal(0, small_int(7))
            .literal(2, small_int(9))
            .bytecodes(&[32, 34])
            .build();

        interpreter.cycle();
        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(7));
        interpreter.cycle();
        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(9));
    }

    #[test]
    fn push_literal_variable_bytecode_pushes_the_associations_value() {
        // 64 = push literal variable 0. Literal 0 holds an Association whose
        // value field is 99; it is the value that must reach the stack, not the
        // Association itself.
        let mut interpreter = Fixture::new()
            .literal(0, ASSOCIATION_OOP)
            .association_value(small_int(99))
            .bytecodes(&[64])
            .build();

        interpreter.cycle();

        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(99));
        assert_ne!(interpreter.stack_top(), OOP::from_raw(ASSOCIATION_OOP));
    }

    #[test]
    fn push_receiver_bytecode_pushes_the_receiver() {
        // 112 = push receiver
        let mut interpreter = Fixture::new().bytecodes(&[112]).build();

        interpreter.cycle();

        assert_eq!(interpreter.stack_top(), OOP::from_raw(RECEIVER_OOP));
        assert_eq!(interpreter.sp, EMPTY_STACK_SP + 1);
    }

    #[test]
    fn push_constant_bytecode_pushes_the_right_constant() {
        for (bytecode, expected) in [
            (113, TRUE_POINTER),
            (114, FALSE_POINTER),
            (115, NIL_POINTER),
            (116, MINUS_ONE_POINTER),
            (117, ZERO_POINTER),
            (118, ONE_POINTER),
            (119, TWO_POINTER),
        ] {
            let mut interpreter = Fixture::new().bytecodes(&[bytecode]).build();
            interpreter.cycle();
            assert_eq!(
                interpreter.stack_top(),
                OOP::from_raw(expected),
                "bytecode {bytecode}"
            );
        }

        // The four numeric constants must decode to the values the book names,
        // which is what catches a transposed entry in the table above.
        for (bytecode, expected) in [(116, -1), (117, 0), (118, 1), (119, 2)] {
            let mut interpreter = Fixture::new().bytecodes(&[bytecode]).build();
            interpreter.cycle();
            assert_eq!(
                interpreter.stack_top().integer_value_of(),
                Ok(expected),
                "bytecode {bytecode}"
            );
        }
    }

    #[test]
    fn push_active_context_bytecode_pushes_the_context() {
        // 137 = push active context, the bytecode compiled for thisContext
        let mut interpreter = Fixture::new().bytecodes(&[137]).build();

        interpreter.cycle();

        assert_eq!(interpreter.stack_top(), OOP::from_raw(CONTEXT_OOP));
    }

    // ┌─────────────────────┐
    // │   STACK OPERATIONS  │
    // └─────────────────────┘

    #[test]
    fn pop_stack_bytecode_drops_the_top() {
        // 112 push receiver, 135 pop
        let mut interpreter = Fixture::new().bytecodes(&[112, 135]).build();

        interpreter.cycle();
        assert_eq!(interpreter.sp, EMPTY_STACK_SP + 1);
        interpreter.cycle();
        assert_eq!(interpreter.sp, EMPTY_STACK_SP);
    }

    #[test]
    fn duplicate_top_bytecode_copies_the_top() {
        // 32 push literal constant 0, 136 duplicate top
        let mut interpreter = Fixture::new()
            .literal(0, small_int(5))
            .bytecodes(&[32, 136])
            .build();

        interpreter.cycle();
        interpreter.cycle();

        assert_eq!(interpreter.sp, EMPTY_STACK_SP + 2);
        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(5));
        assert_eq!(interpreter.stack_value(1).integer_value_of(), Ok(5));
    }

    // ┌─────────────────────┐
    // │       STORES        │
    // └─────────────────────┘

    #[test]
    fn store_and_pop_receiver_variable_bytecode_stores_and_pops() {
        // 32 push literal constant 0, 96 store and pop receiver variable 0
        let mut interpreter = Fixture::new()
            .literal(0, small_int(77))
            .bytecodes(&[32, 96])
            .build();

        interpreter.cycle();
        interpreter.cycle();

        assert_eq!(interpreter.sp, EMPTY_STACK_SP);
        assert_eq!(
            field_of(&interpreter, RECEIVER_OOP, 0).integer_value_of(),
            Ok(77)
        );
    }

    #[test]
    fn store_and_pop_receiver_variable_bytecode_reaches_the_last_variable() {
        // 103 is the last of the eight, addressing instance variable 7. Unlike
        // the pushes, instance variables are not displaced by a frame offset, so
        // the field index is the variable index.
        let mut interpreter = Fixture::new()
            .literal(0, small_int(77))
            .bytecodes(&[32, 103])
            .build();

        interpreter.cycle();
        interpreter.cycle();

        assert_eq!(
            field_of(&interpreter, RECEIVER_OOP, 7).integer_value_of(),
            Ok(77)
        );
    }

    #[test]
    fn store_and_pop_temporary_variable_bytecode_stores_into_the_temp_frame() {
        // 32 push literal constant 0, 104 store and pop temporary 0.
        //
        // The temp frame starts at TempFrameStart, so temporary 0 is that field
        // of the context. This also pins the width of the index field: 104..111
        // occupy 0x68..0x6F, so a four-bit index would decode 104 as temporary 8
        // and the value would land in the evaluation stack instead.
        let mut interpreter = Fixture::new()
            .literal(0, small_int(88))
            .bytecodes(&[32, 104])
            .build();

        interpreter.cycle();
        interpreter.cycle();

        assert_eq!(interpreter.sp, EMPTY_STACK_SP);
        assert_eq!(
            field_of(&interpreter, CONTEXT_OOP, TEMP_FRAME_START).integer_value_of(),
            Ok(88)
        );
    }

    #[test]
    fn store_and_pop_temporary_variable_round_trips_through_a_push() {
        // 32 push literal constant 0, 107 store and pop temporary 3,
        // 19 push temporary 3. Reader and writer have to agree about where the
        // temp frame begins, which asserting on a raw field index alone cannot
        // establish.
        let mut interpreter = Fixture::new()
            .literal(0, small_int(55))
            .bytecodes(&[32, 107, 19])
            .build();

        interpreter.cycle();
        interpreter.cycle();
        interpreter.cycle();

        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(55));
        assert_eq!(interpreter.sp, EMPTY_STACK_SP + 1);
    }

    #[test]
    fn store_and_pop_temporary_variable_leaves_the_fixed_context_fields_alone() {
        // Without the TempFrameStart offset, temporary 0 would land on the
        // context's sender field, temporary 1 on the instruction pointer and so
        // on, corrupting the return chain rather than failing loudly.
        let mut interpreter = Fixture::new()
            .literal(0, small_int(55))
            .bytecodes(&[32, 104])
            .build();

        interpreter.cycle();
        interpreter.cycle();

        assert_eq!(
            field_of(&interpreter, CONTEXT_OOP, SENDER_INDEX),
            OOP::from_raw(NIL_POINTER)
        );
        assert_eq!(
            field_of(&interpreter, CONTEXT_OOP, METHOD_INDEX),
            OOP::from_raw(METHOD_OOP)
        );
        assert_eq!(
            field_of(&interpreter, CONTEXT_OOP, RECEIVER_INDEX),
            OOP::from_raw(RECEIVER_OOP)
        );
    }

    #[test]
    fn stores_read_back_through_the_matching_pushes() {
        // Every short store paired with the push that reads the same location:
        // 96..103 against 0..15 for the receiver, 104..111 against 16..31 for
        // the temp frame.
        for index in 0..SHORT_STORE_COUNT {
            let mut interpreter = Fixture::new()
                .literal(0, small_int(100 + index as u16))
                .bytecodes(&[32, 96 + index, index])
                .build();

            interpreter.cycle();
            interpreter.cycle();
            interpreter.cycle();

            assert_eq!(
                interpreter.stack_top().integer_value_of(),
                Ok(100 + index as i16),
                "receiver variable {index}"
            );
        }

        for index in 0..SHORT_STORE_COUNT {
            let mut interpreter = Fixture::new()
                .literal(0, small_int(200 + index as u16))
                .bytecodes(&[32, 104 + index, 16 + index])
                .build();

            interpreter.cycle();
            interpreter.cycle();
            interpreter.cycle();

            assert_eq!(
                interpreter.stack_top().integer_value_of(),
                Ok(200 + index as i16),
                "temporary {index}"
            );
        }
    }

    #[test]
    fn instance_variable_setter_is_visible_to_the_push() {
        // Guards the fixture itself: instance_variable writes where
        // pushReceiverVariable reads.
        let mut interpreter = Fixture::new()
            .instance_variable(5, small_int(123))
            .bytecodes(&[5])
            .build();

        interpreter.cycle();

        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(123));
    }

    // ┌─────────────────────┐
    // │   EXTENDED FORMS    │
    // └─────────────────────┘

    /// Builds the descriptor byte that follows an extended push or store: the
    /// variable type in the high two bits, the index in the low six.
    fn descriptor(variable_type: u8, index: u8) -> u8 {
        (variable_type << 6) | index
    }

    /// Index of a receiver variable or temporary that only the extended forms
    /// can address; the short pushes stop at 15.
    const BEYOND_SHORT_VARIABLE: u16 = 17;

    /// Index of a literal that only the extended forms can address; the short
    /// literal pushes stop at 31.
    const BEYOND_SHORT_LITERAL: u16 = 33;

    #[test]
    fn extended_push_bytecode_pushes_a_receiver_variable_beyond_the_short_range() {
        // 128 extended push, descriptor type 0, index 17
        let mut interpreter = Fixture::new()
            .instance_variable(BEYOND_SHORT_VARIABLE, small_int(42))
            .bytecodes(&[128, descriptor(0, BEYOND_SHORT_VARIABLE as u8)])
            .build();

        interpreter.cycle();

        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(42));
        // The descriptor byte is consumed along with the bytecode.
        assert_eq!(interpreter.ip, FIRST_BYTECODE_BYTE as usize + 2);
    }

    #[test]
    fn extended_push_bytecode_pushes_a_temporary_beyond_the_short_range() {
        // 128 extended push, descriptor type 1, index 17
        let mut interpreter = Fixture::new()
            .temporary(BEYOND_SHORT_VARIABLE, small_int(43))
            .bytecodes(&[128, descriptor(1, BEYOND_SHORT_VARIABLE as u8)])
            .build();

        interpreter.cycle();

        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(43));
    }

    #[test]
    fn extended_push_bytecode_pushes_a_literal_constant_beyond_the_short_range() {
        // 128 extended push, descriptor type 2, index 33
        let mut interpreter = Fixture::new()
            .literal(BEYOND_SHORT_LITERAL, small_int(44))
            .bytecodes(&[128, descriptor(2, BEYOND_SHORT_LITERAL as u8)])
            .build();

        interpreter.cycle();

        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(44));
    }

    #[test]
    fn extended_push_bytecode_pushes_a_literal_variables_value() {
        // 128 extended push, descriptor type 3, index 33. As with the short
        // form, the Association's value is pushed rather than the Association.
        let mut interpreter = Fixture::new()
            .literal(BEYOND_SHORT_LITERAL, ASSOCIATION_OOP)
            .association_value(small_int(45))
            .bytecodes(&[128, descriptor(3, BEYOND_SHORT_LITERAL as u8)])
            .build();

        interpreter.cycle();

        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(45));
        assert_ne!(interpreter.stack_top(), OOP::from_raw(ASSOCIATION_OOP));
    }

    #[test]
    fn extended_push_agrees_with_the_short_forms_for_low_indices() {
        // For an index both forms can address, the extended push has to land on
        // the same variable as the single byte push it generalises. Index 3 for
        // each of the three types that address a numbered frame.
        for (variable_type, short_bytecode) in [(0u8, 3u8), (1, 16 + 3), (2, 32 + 3)] {
            let setup = |bytes: &[u8]| {
                Fixture::new()
                    .instance_variable(3, small_int(10))
                    .temporary(3, small_int(20))
                    .literal(3, small_int(30))
                    .bytecodes(bytes)
                    .build()
            };

            let mut short = setup(&[short_bytecode]);
            let mut extended = setup(&[128, descriptor(variable_type, 3)]);
            short.cycle();
            extended.cycle();

            assert_eq!(
                short.stack_top(),
                extended.stack_top(),
                "variable type {variable_type}"
            );
        }
    }

    #[test]
    fn extended_store_bytecode_stores_a_receiver_variable_and_keeps_the_value() {
        // 32 push literal constant 0, 129 extended store, descriptor type 0
        let mut interpreter = Fixture::new()
            .literal(0, small_int(64))
            .bytecodes(&[32, 129, descriptor(0, BEYOND_SHORT_VARIABLE as u8)])
            .build();

        interpreter.cycle();
        interpreter.cycle();

        assert_eq!(
            field_of(&interpreter, RECEIVER_OOP, BEYOND_SHORT_VARIABLE).integer_value_of(),
            Ok(64)
        );
        // The extended store leaves the value on the stack; only 130 pops it.
        assert_eq!(interpreter.sp, EMPTY_STACK_SP + 1);
        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(64));
    }

    #[test]
    fn extended_store_bytecode_stores_a_temporary_and_keeps_the_value() {
        // 32 push literal constant 0, 129 extended store, descriptor type 1
        let mut interpreter = Fixture::new()
            .literal(0, small_int(65))
            .bytecodes(&[32, 129, descriptor(1, BEYOND_SHORT_VARIABLE as u8)])
            .build();

        interpreter.cycle();
        interpreter.cycle();

        assert_eq!(
            field_of(
                &interpreter,
                CONTEXT_OOP,
                TEMP_FRAME_START + BEYOND_SHORT_VARIABLE
            )
            .integer_value_of(),
            Ok(65)
        );
        assert_eq!(interpreter.sp, EMPTY_STACK_SP + 1);
    }

    #[test]
    fn extended_store_bytecode_stores_through_an_association_and_keeps_the_value() {
        // 32 push literal constant 0, 129 extended store, descriptor type 3.
        // The store lands in the Association's value field, which is how an
        // assignment to a global reaches every method that reads it.
        let mut interpreter = Fixture::new()
            .literal(0, small_int(66))
            .literal(BEYOND_SHORT_LITERAL, ASSOCIATION_OOP)
            .bytecodes(&[32, 129, descriptor(3, BEYOND_SHORT_LITERAL as u8)])
            .build();

        interpreter.cycle();
        interpreter.cycle();

        assert_eq!(
            field_of(&interpreter, ASSOCIATION_OOP, VALUE_INDEX).integer_value_of(),
            Ok(66)
        );
        assert_eq!(interpreter.sp, EMPTY_STACK_SP + 1);
    }

    #[test]
    #[should_panic(expected = "illegal store into a literal constant")]
    fn extended_store_bytecode_rejects_a_store_into_a_literal_constant() {
        // Descriptor type 2 would mean assigning to a literal constant. The
        // compiler never emits it, so reaching it means the method is malformed.
        let mut interpreter = Fixture::new()
            .literal(0, small_int(1))
            .bytecodes(&[32, 129, descriptor(2, 0)])
            .build();

        interpreter.cycle();
        interpreter.cycle();
    }

    #[test]
    fn extended_store_and_pop_bytecode_stores_pops_and_consumes_two_bytes() {
        // 32 push literal constant 0, 130 extended store and pop, type 1.
        //
        // The ip assertion is the point: 130 is specified as an extended store
        // followed by a pop, and wiring that second step to anything that fetches
        // a byte of its own would consume three bytes here, desyncing every
        // bytecode after it while still storing the right value.
        let mut interpreter = Fixture::new()
            .literal(0, small_int(70))
            .bytecodes(&[32, 130, descriptor(1, BEYOND_SHORT_VARIABLE as u8)])
            .build();

        interpreter.cycle();
        let after_push = interpreter.ip;
        interpreter.cycle();

        assert_eq!(
            field_of(
                &interpreter,
                CONTEXT_OOP,
                TEMP_FRAME_START + BEYOND_SHORT_VARIABLE
            )
            .integer_value_of(),
            Ok(70)
        );
        assert_eq!(interpreter.sp, EMPTY_STACK_SP, "the value must be popped");
        assert_eq!(
            interpreter.ip,
            after_push + 2,
            "bytecode plus descriptor only"
        );
    }

    #[test]
    fn extended_store_and_pop_round_trips_through_an_extended_push() {
        // 32 push literal constant 0, 130 store and pop to receiver variable 17,
        // 128 push it back. Both extended forms have to agree about where the
        // descriptor's index points.
        let mut interpreter = Fixture::new()
            .literal(0, small_int(71))
            .bytecodes(&[
                32,
                130,
                descriptor(0, BEYOND_SHORT_VARIABLE as u8),
                128,
                descriptor(0, BEYOND_SHORT_VARIABLE as u8),
            ])
            .build();

        interpreter.cycle();
        interpreter.cycle();
        interpreter.cycle();

        assert_eq!(interpreter.stack_top().integer_value_of(), Ok(71));
        assert_eq!(interpreter.sp, EMPTY_STACK_SP + 1);
    }
}

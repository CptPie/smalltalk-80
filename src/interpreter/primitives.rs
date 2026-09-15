//! The primitive methods, Bluebook chapter 29.
//!
//! A child module of the interpreter rather than a sibling so that it can reach
//! the interpreter's private registers and stack helpers without widening their
//! visibility. Everything here is an `impl Interpreter` block; the split is
//! organisational, matching the book's chapter boundary.
//!
//! The book tracks primitive failure in a `success` register that every helper
//! ANDs into, and each primitive restores the stack itself with `unPop:` when
//! the register ends up false. That shape exists because Smalltalk-80 has no
//! early return from the middle of a method. Rust does, so failure is a
//! `Result` propagated with `?`, and the stack is restored in one place: the
//! entry point saves the stack pointer before dispatching and puts it back on
//! `Err`. No primitive needs to know how much it has popped.

use crate::{
    globalconstants::{
        CLASS_BLOCK_CONTEXT_POINTER, CLASS_METHOD_CONTEXT_POINTER, CLASS_POINT_POINTER,
        FALSE_POINTER, SPECIAL_SELECTORS_ARRAY_POINTER, TRUE_POINTER,
    },
    oop::OOP,
};

use super::Interpreter;

/// A primitive could not produce a result, and the Smalltalk method it fronts
/// has to run instead.
///
/// Carries no reason on purpose: the image never learns why a primitive failed,
/// only that it did, so a reason would be information nothing consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PrimitiveFailure;

/// What every primitive routine answers. `Ok` means the result is on the stack
/// and the send is complete.
pub(super) type PrimitiveResult = Result<(), PrimitiveFailure>;

// Point layout, for primitiveMakePoint.
//
//  see Bluebook p. 625
//
//  initializePointIndices
//    XIndex ← 0.
//    YIndex ← 1.
//    ClassPointSize ← 2
const X_INDEX: u16 = 0;
const Y_INDEX: u16 = 1;
const CLASS_POINT_SIZE: u16 = 2;

// ====================================
//  Entry points, called from chapter 28
// ====================================

impl Interpreter {
    /// Attempts to answer the current message with a primitive routine.
    ///
    /// Reached after a message lookup has produced `new_method`. Two shortcuts
    /// come first, keyed on the method header's flag value: a method that only
    /// returns self, and one that only returns an instance variable, need no
    /// primitive index and no context. Everything else goes by index.
    ///
    /// # Returns:
    ///     * bool, true if the primitive produced a result and the send is complete
    //
    //  see Bluebook p. 620
    //
    //  primitiveResponse
    //    | flagValue thisReceiver offset |
    //    primitiveIndex = 0
    //      ifTrue: [flagValue ← self flagValueOf: newMethod.
    //              flagValue = 5
    //                ifTrue: [self quickReturnSelf.
    //                        ↑true].
    //              flagValue = 6
    //                ifTrue: [self quickInstanceLoad.
    //                        ↑true].
    //              ↑false]
    //      ifFalse: [self initPrimitive.
    //               self dispatchPrimitives.
    //               ↑self success]
    //
    // initPrimitive and the success register are replaced by the Result the
    // dispatch returns. Save sp before dispatching and restore it on Err: that
    // is every primitive's unPop: done once, and it cannot get the count wrong.
    pub(super) fn primitive_response(&mut self) -> bool {
        if self.primitive_index == 0 {
            let flag_value = self.flag_value_of(self.new_method);
            if flag_value == 5 {
                self.quick_return_self();
                return true;
            }
            if flag_value == 6 {
                self.quick_instance_load();
                return true;
            }
            return false;
        } else {
            let sp = self.sp;
            match self.dispatch_primitives() {
                Ok(()) => true,
                Err(PrimitiveFailure) => {
                    self.sp = sp;
                    false
                }
            }
        }
    }

    /// Answers a send-special-selector bytecode without a message lookup.
    ///
    /// Only some selectors qualify, and only for some receivers: the arithmetic
    /// selectors when the receiver is a SmallInteger, and five of the common
    /// selectors under conditions of their own. Anything else reports failure,
    /// and the bytecode falls back to an ordinary send.
    ///
    /// # Returns:
    ///     * bool, true if the primitive produced a result and the send is complete
    //
    //  see Bluebook p. 618
    //
    //  specialSelectorPrimitiveResponse
    //    self initPrimitive.
    //    (currentBytecode between: 176 and: 191)
    //      ifTrue: [self arithmeticSelectorPrimitive].
    //    (currentBytecode between: 192 and: 207)
    //      ifTrue: [self commonSelectorPrimitive].
    //    ↑self success
    //
    // Same sp save and restore as primitive_response. Reports failure until
    // written, for the same reason.
    pub(super) fn special_selector_primitive_response(&mut self) -> bool {
        false
    }
}

// ====================================
//  Quick returns, by header flag
// ====================================

impl Interpreter {
    /// Returns self without activating the method.
    ///
    /// A no-op: the receiver already sits on the stack exactly where the result
    /// of the send belongs.
    //
    //  see Bluebook p. 620
    //
    //  quickReturnSelf
    fn quick_return_self(&mut self) {}

    /// Returns one of the receiver's instance variables without activating the
    /// method. Which one is in the method header's field index.
    //
    //  see Bluebook p. 620
    //
    //  quickInstanceLoad
    //    | thisReceiver fieldIndex |
    //    thisReceiver ← self popStack.
    //    fieldIndex ← self fieldIndexOf: newMethod.
    //    self push: (memory fetchPointer: fieldIndex
    //                       ofObject: thisReceiver)
    fn quick_instance_load(&mut self) {
        todo!()
    }
}

// ====================================
//  Dispatch
// ====================================

impl Interpreter {
    /// Runs the primitive named by `primitive_index`.
    ///
    /// # Returns:
    ///     * PrimitiveResult, Ok if the result is on the stack
    //
    //  see Bluebook p. 621
    //
    //  dispatchPrimitives
    //    primitiveIndex < 60
    //      ifTrue: [↑self dispatchArithmeticPrimitives].
    //    primitiveIndex < 68
    //      ifTrue: [↑self dispatchSubscriptAndStreamPrimitives].
    //    primitiveIndex < 80
    //      ifTrue: [↑self dispatchStorageManagementPrimitives].
    //    primitiveIndex < 90
    //      ifTrue: [↑self dispatchControlPrimitives].
    //    primitiveIndex < 110
    //      ifTrue: [↑self dispatchInputOutputPrimitives].
    //    primitiveIndex < 128
    //      ifTrue: [↑self dispatchSystemPrimitives].
    //    primitiveIndex < 256
    //      ifTrue: [↑self dispatchPrivatePrimitives]
    //
    // The book's two levels of range tests are an artifact of writing a jump
    // table as nested if-chains, the same as the bytecode dispatch was. One flat
    // match over the index is that table. An index this implementation does not
    // provide is an ordinary failure, not an error: the image falls back to the
    // Smalltalk method, which is what the book prescribes for an optional
    // primitive (p. 625, dispatchLargeIntegerPrimitives is just primitiveFail).
    fn dispatch_primitives(&mut self) -> PrimitiveResult {
        match self.primitive_index {
            1 => self.primitive_add(),
            2 => self.primitive_subtract(),
            3 => self.primitive_less_than(),
            4 => self.primitive_greater_than(),
            5 => self.primitive_less_or_equal(),
            6 => self.primitive_greater_or_equal(),
            7 => self.primitive_equal(),
            8 => self.primitive_not_equal(),
            9 => self.primitive_multiply(),
            10 => self.primitive_divide(),
            11 => self.primitive_mod(),
            12 => self.primitive_div(),
            13 => self.primitive_quo(),
            14 => self.primitive_bit_and(),
            15 => self.primitive_bit_or(),
            16 => self.primitive_bit_xor(),
            17 => self.primitive_bit_shift(),
            18 => self.primitive_make_point(),

            110 => self.primitive_equivalent(),
            111 => self.primitive_class(),

            _ => Err(PrimitiveFailure),
        }
    }

    /// Answers an arithmetic special selector bytecode (176 through 191) when
    /// the receiver is a SmallInteger.
    ///
    /// The receiver is the second stack value: the argument is on top.
    //
    //  see Bluebook p. 619
    //
    //  arithmeticSelectorPrimitive
    //    self success: (memory isIntegerObject: (self stackValue: 1)).
    //    self success
    //      ifTrue: [currentBytecode = 176 ifTrue: [↑self primitiveAdd].
    //              currentBytecode = 177 ifTrue: [↑self primitiveSubtract].
    //              currentBytecode = 178 ifTrue: [↑self primitiveLessThan].
    //              currentBytecode = 179 ifTrue: [↑self primitiveGreaterThan].
    //              currentBytecode = 180 ifTrue: [↑self primitiveLessOrEqual].
    //              currentBytecode = 181 ifTrue: [↑self primitiveGreaterOrEqual].
    //              currentBytecode = 182 ifTrue: [↑self primitiveEqual].
    //              currentBytecode = 183 ifTrue: [↑self primitiveNotEqual].
    //              currentBytecode = 184 ifTrue: [↑self primitiveMultiply].
    //              currentBytecode = 185 ifTrue: [↑self primitiveDivide].
    //              currentBytecode = 186 ifTrue: [↑self primitiveMod].
    //              currentBytecode = 187 ifTrue: [↑self primitiveMakePoint].
    //              currentBytecode = 188 ifTrue: [↑self primitiveBitShift].
    //              currentBytecode = 189 ifTrue: [↑self primitiveDiv].
    //              currentBytecode = 190 ifTrue: [↑self primitiveBitAnd].
    //              currentBytecode = 191 ifTrue: [↑self primitiveBitOr]]
    //
    // Note the order is the special selector array's, not the primitive index
    // order: 187 is @, 188 is bitShift:, 189 is //.
    fn arithmetic_selector_primitive(&mut self) -> PrimitiveResult {
        todo!()
    }

    /// Answers one of the five common special selectors (192 through 207) that
    /// have a primitive: ==, class, blockCopy:, value and value:.
    ///
    /// == and class answer for any receiver. blockCopy: needs a context as
    /// receiver, value and value: need a BlockContext. The receiver sits below
    /// the arguments, so its position depends on the selector's argument count,
    /// which the special selector array records next to each selector.
    //
    //  see Bluebook p. 619
    //
    //  commonSelectorPrimitive
    //    | receiverClass |
    //    argumentCount ← self fetchInteger: (currentBytecode - 176) * 2 + 1
    //                         ofObject: SpecialSelectorsPointer.
    //    receiverClass ←
    //      memory fetchClassOf: (self stackValue: argumentCount).
    //    currentBytecode = 198 ifTrue: [↑self primitiveEquivalent].
    //    currentBytecode = 199 ifTrue: [↑self primitiveClass].
    //    currentBytecode = 200
    //      ifTrue: [self success:
    //                (receiverClass = ClassMethodContextPointer)
    //                  | (receiverClass = ClassBlockContextPointer).
    //              ↑self success ifTrue: [self primitiveBlockCopy]].
    //    (currentBytecode = 201) | (currentBytecode = 202)
    //      ifTrue: [self success: receiverClass = ClassBlockContextPointer.
    //              ↑self success ifTrue: [self primitiveValue]].
    //    self primitiveFail
    //
    // blockCopy:, value and value: are control primitives (80, 81) and come
    // with that section. Until then 200 through 202 fail here, which sends the
    // message for real, which is also what the book does for any receiver of
    // the wrong class.
    fn common_selector_primitive(&mut self) -> PrimitiveResult {
        todo!()
    }
}

// ====================================
//  Integer helpers
// ====================================

impl Interpreter {
    /// Pops the top of the stack, which has to be a SmallInteger.
    ///
    /// # Returns:
    ///     * Result<i16, PrimitiveFailure>, the value, or failure if the top was
    ///       not a SmallInteger
    //
    //  see Bluebook p. 617
    //
    //  popInteger
    //    | integerPointer |
    //    integerPointer ← self popStack.
    //    self success: (memory isIntegerObject: integerPointer).
    //    self success
    //      ifTrue: [↑memory integerValueOf: integerPointer]
    //
    // The pop happens whether or not it succeeds, the same as the book, since
    // the caller's stack pointer is restored wholesale on failure.
    fn pop_integer(&mut self) -> Result<i16, PrimitiveFailure> {
        todo!()
    }

    /// Pushes a value as a SmallInteger.
    ///
    /// # Parameters:
    ///     * value (i16): The value to push
    ///
    /// # Returns:
    ///     * PrimitiveResult, failure if the value does not fit a SmallInteger
    //
    //  see Bluebook p. 617
    //
    //  pushInteger: integerValue
    //    self push: (memory integerObjectOf: integerValue)
    //
    // The book's integerObjectOf: assumes the value fits, and every caller
    // checks isIntegerValue: first. Folding that check in here means an
    // arithmetic primitive whose result overflowed fails at the push, through
    // the same ? as everything else, instead of each one testing by hand.
    fn push_integer(&mut self, value: i16) -> PrimitiveResult {
        todo!()
    }
}

// ====================================
//  Arithmetic primitives, 1 through 18
// ====================================

impl Interpreter {
    /// Adds two SmallIntegers.
    //
    //  see Bluebook p. 622
    //
    //  primitiveAdd
    //    | integerReceiver integerArgument integerResult |
    //    integerArgument ← self popInteger.
    //    integerReceiver ← self popInteger.
    //    self success
    //      ifTrue: [integerResult ← integerReceiver + integerArgument.
    //              self success: (memory isIntegerValue: integerResult)].
    //    self success
    //      ifTrue: [self pushInteger: integerResult]
    //      ifFalse: [self unPop: 2]
    //
    // The argument comes off first: it was pushed last. i16 arithmetic can
    // overflow on its own before the SmallInteger range check ever sees the
    // result, so use the checked_ forms and treat None as failure too.
    fn primitive_add(&mut self) -> PrimitiveResult {
        todo!()
    }

    /// Subtracts the argument from the receiver.
    //
    //  see Bluebook p. 622: identical to primitiveAdd but for the operation.
    fn primitive_subtract(&mut self) -> PrimitiveResult {
        todo!()
    }

    /// Multiplies two SmallIntegers.
    //
    //  see Bluebook p. 622: identical to primitiveAdd but for the operation.
    fn primitive_multiply(&mut self) -> PrimitiveResult {
        todo!()
    }

    /// Divides, answering only when the division is exact.
    //
    //  see Bluebook p. 622
    //
    //  primitiveDivide
    //    | integerReceiver integerArgument integerResult |
    //    integerArgument ← self popInteger.
    //    integerReceiver ← self popInteger.
    //    self success: integerArgument ~= 0.
    //    self success: integerReceiver \\ integerArgument = 0.
    //    self success
    //      ifTrue: [integerResult ← integerReceiver // integerArgument.
    //              self success: (memory isIntegerValue: integerResult)].
    //    self success
    //      ifTrue: [self push: (memory integerObjectOf: integerResult)]
    //      ifFalse: [self unPop: 2]
    fn primitive_divide(&mut self) -> PrimitiveResult {
        todo!()
    }

    /// The remainder of a division whose quotient rounds toward negative
    /// infinity, so the result takes the sign of the argument.
    //
    //  see Bluebook p. 623
    //
    //  primitiveMod
    //    | integerReceiver integerArgument integerResult |
    //    integerArgument ← self popInteger.
    //    integerReceiver ← self popInteger.
    //    self success: integerArgument ~= 0.
    //    self success
    //      ifTrue: [integerResult ← integerReceiver \\ integerArgument.
    //              self success: (memory isIntegerValue: integerResult)].
    //    self success
    //      ifTrue: [self pushInteger: integerResult]
    //      ifFalse: [self unPop: 2]
    //
    // Smalltalk's \\ is floored modulo. Rust's % truncates toward zero and
    // takes the sign of the dividend, so -7 \\ 2 is 1 in Smalltalk and -7 % 2
    // is -1 in Rust. i16::rem_euclid is not it either, that is always
    // non-negative. Derive it from the floored quotient below.
    fn primitive_mod(&mut self) -> PrimitiveResult {
        todo!()
    }

    /// The quotient of a division rounded toward negative infinity.
    //
    //  see Bluebook p. 623
    //
    //  primitiveDiv
    //    | integerReceiver integerArgument integerResult |
    //    integerArgument ← self popInteger.
    //    integerReceiver ← self popInteger.
    //    self success: integerArgument ~= 0.
    //    self success
    //      ifTrue: [integerResult ← integerReceiver // integerArgument.
    //              self success: (memory isIntegerValue: integerResult)].
    //    self success
    //      ifTrue: [self pushInteger: integerResult]
    //      ifFalse: [self unPop: 2]
    //
    // Rust's / truncates toward zero, so -7 // 2 is -4 in Smalltalk and -7 / 2
    // is -3 in Rust. i16::div_euclid rounds toward negative infinity only for a
    // positive divisor; for the general case, truncate and then subtract one if
    // there was a remainder and the signs differed.
    fn primitive_div(&mut self) -> PrimitiveResult {
        todo!()
    }

    /// The quotient of a division truncated toward zero, which is the one Rust's
    /// / already does.
    //
    //  see Bluebook p. 623
    //
    //  primitiveQuo
    //    | integerReceiver integerArgument integerResult |
    //    integerArgument ← self popInteger.
    //    integerReceiver ← self popInteger.
    //    self success: integerArgument ~= 0.
    //    self success
    //      ifTrue: [integerResult ← integerReceiver quo: integerArgument.
    //              self success: (memory isIntegerValue: integerResult)].
    //    self success
    //      ifTrue: [self pushInteger: integerResult]
    //      ifFalse: [self unPop: 2]
    fn primitive_quo(&mut self) -> PrimitiveResult {
        todo!()
    }

    /// Compares two SmallIntegers for equality, pushing true or false.
    //
    //  see Bluebook p. 624
    //
    //  primitiveEqual
    //    | integerReceiver integerArgument integerResult |
    //    integerArgument ← self popInteger.
    //    integerReceiver ← self popInteger.
    //    self success
    //      ifTrue: [integerReceiver = integerArgument
    //                ifTrue: [self push: TruePointer]
    //                ifFalse: [self push: FalsePointer]]
    //      ifFalse: [self unPop: 2]
    //
    // The comparisons push a Boolean, not an integer, so they cannot overflow
    // and do not go through push_integer.
    fn primitive_equal(&mut self) -> PrimitiveResult {
        todo!()
    }

    //  see Bluebook p. 624: identical to primitiveEqual but for the comparison.
    fn primitive_not_equal(&mut self) -> PrimitiveResult {
        todo!()
    }

    //  see Bluebook p. 624: identical to primitiveEqual but for the comparison.
    fn primitive_less_than(&mut self) -> PrimitiveResult {
        todo!()
    }

    //  see Bluebook p. 624: identical to primitiveEqual but for the comparison.
    fn primitive_greater_than(&mut self) -> PrimitiveResult {
        todo!()
    }

    //  see Bluebook p. 624: identical to primitiveEqual but for the comparison.
    fn primitive_less_or_equal(&mut self) -> PrimitiveResult {
        todo!()
    }

    //  see Bluebook p. 624: identical to primitiveEqual but for the comparison.
    fn primitive_greater_or_equal(&mut self) -> PrimitiveResult {
        todo!()
    }

    /// Bitwise and of two SmallIntegers, on their two's complement
    /// representations.
    //
    //  see Bluebook p. 624
    //
    //  primitiveBitAnd
    //    | integerReceiver integerArgument integerResult |
    //    integerArgument ← self popInteger.
    //    integerReceiver ← self popInteger.
    //    self success
    //      ifTrue: [integerResult ← integerReceiver bitAnd: integerArgument].
    //    self success
    //      ifTrue: [self pushInteger: integerResult]
    //      ifFalse: [self unPop: 2]
    //
    // Bitwise operations on two values in range produce a value in range, so
    // the book does not check. push_integer checks anyway, which costs nothing.
    fn primitive_bit_and(&mut self) -> PrimitiveResult {
        todo!()
    }

    //  see Bluebook p. 624: identical to primitiveBitAnd but for the operation.
    fn primitive_bit_or(&mut self) -> PrimitiveResult {
        todo!()
    }

    //  see Bluebook p. 624: identical to primitiveBitAnd but for the operation.
    fn primitive_bit_xor(&mut self) -> PrimitiveResult {
        todo!()
    }

    /// Shifts the receiver left by the argument, or right for a negative
    /// argument. Left shifts fill with zeros; right shifts extend the sign.
    /// Fails if the result does not fit a SmallInteger.
    //
    //  see Bluebook p. 624
    //
    //  primitiveBitShift
    //    | integerReceiver integerArgument integerResult |
    //    integerArgument ← self popInteger.
    //    integerReceiver ← self popInteger.
    //    self success
    //      ifTrue: [integerResult ← integerReceiver bitShift: integerArgument.
    //              self success: (memory isIntegerValue: integerResult)].
    //    self success
    //      ifTrue: [self pushInteger: integerResult]
    //      ifFalse: [self unPop: 2]
    //
    // A left shift by 15 or more of anything but zero cannot fit, and a Rust
    // shift by the type's width or more panics in debug builds. Widen to i32
    // for the shift and let push_integer decide whether the result fits, or
    // fail outright for shift amounts at or beyond 16.
    fn primitive_bit_shift(&mut self) -> PrimitiveResult {
        todo!()
    }

    /// Makes a Point with the receiver as x and the argument as y.
    //
    //  see Bluebook p. 625
    //
    //  primitiveMakePoint
    //    | integerReceiver integerArgument pointResult |
    //    integerArgument ← self popStack.
    //    integerReceiver ← self popStack.
    //    self success: (memory isIntegerValue: integerReceiver).
    //    self success: (memory isIntegerValue: integerArgument).
    //    self success
    //      ifTrue: [pointResult ← memory instantiateClass: ClassPointPointer
    //                                      withPointers: ClassPointSize.
    //              memory storePointer: XIndex
    //                     ofObject: pointResult
    //                     withValue: integerReceiver.
    //              memory storePointer: YIndex
    //                     ofObject: pointResult
    //                     withValue: integerArgument.
    //              self push: pointResult]
    //      ifFalse: [self unPop: 2]
    //
    // Note the book pops with popStack and tests with isIntegerValue:, which is
    // checking the wrong thing (a pointer, against the value range). Testing
    // isIntegerObject: is what it means. The fields are stored as the
    // SmallInteger pointers themselves, not their values.
    fn primitive_make_point(&mut self) -> PrimitiveResult {
        todo!()
    }
}

// ====================================
//  System primitives, 110 and 111
// ====================================

impl Interpreter {
    /// Identity: true if the receiver and argument are the same object.
    //
    //  see Bluebook p. 653
    //
    //  primitiveEquivalent
    //    | thisObject otherObject |
    //    otherObject ← self popStack.
    //    thisObject ← self popStack.
    //    thisObject = otherObject
    //      ifTrue: [self push: TruePointer]
    //      ifFalse: [self push: FalsePointer]
    //
    // Comparing pointers is the whole definition, which is why this works for
    // SmallIntegers too: equal values have equal tagged pointers.
    fn primitive_equivalent(&mut self) -> PrimitiveResult {
        todo!()
    }

    /// The class of the receiver.
    //
    //  see Bluebook p. 653
    //
    //  primitiveClass
    //    | instance |
    //    instance ← self popStack.
    //    self push: (memory fetchClassOf: instance)
    fn primitive_class(&mut self) -> PrimitiveResult {
        todo!()
    }
}

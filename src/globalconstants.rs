/// Size (in words) of an object header
pub const HEADER_SIZE: u16 = 2;

// Well known object pointers

/// The nil pointer
pub const NIL_POINTER: u16 = 0x02;

/// Pointer to the 'false' object
pub const FALSE_POINTER: u16 = 0x04;

/// Pointer to the 'true' object
pub const TRUE_POINTER: u16 = 0x06;

/// Pointer to the process scheduler
pub const SCHEDULER_ASSOCIATION_POINTER: u16 = 0x08;

/// Pointer to the SmallInteger class
pub const CLASS_SMALL_INTEGER_POINTER: u16 = 0x0C;

/// Pointer to the String class
pub const CLASS_STRING_POINTER: u16 = 0x0E;

/// Pointer to the Array class
pub const CLASS_ARRAY_POINTER: u16 = 0x10;

/// Pointer to the Float class
pub const CLASS_FLOAT_POINTER: u16 = 0x14;

/// Pointer to the MethodContext class
pub const CLASS_METHOD_CONTEXT_POINTER: u16 = 0x16;

/// Pointer to the BlockContext class
pub const CLASS_BLOCK_CONTEXT_POINTER: u16 = 0x18;

/// Pointer to the Point class
pub const CLASS_POINT_POINTER: u16 = 0x1A;

/// Pointer to the LargePositiveInteger class
pub const CLASS_LARGE_POSITIVE_INTEGER_POINTER: u16 = 0x1C;

/// Pointer to the Message class
pub const CLASS_MESSAGE_POINTER: u16 = 0x20;

/// Pointer to the CompiledMethod class
pub const CLASS_COMPILED_METHOD_POINTER: u16 = 0x22;

/// Pointer to the Character class
pub const CLASS_CHARACTER_POINTER: u16 = 0x28;

/// Pointer to the special selectors array
pub const SPECIAL_SELECTORS_ARRAY_POINTER: u16 = 0x30;

/// Pointer to the character table
pub const CHARACTER_TABLE_POINTER: u16 = 0x38;

// Selectors

/// Selector for #doesNotUnderstand
pub const DOES_NOT_UNDERSTAND_SELECTOR: u16 = 0x2A;

/// Selector for #cannotReturn
pub const CANNOT_RETURN_SELECTOR: u16 = 0x2C;

/// Selector for #mustBeBoolean
pub const MUST_BE_BOOLEAN_SELECTOR: u16 = 0x34;

// Allocation variables
pub const NON_POINTER: u16 = 0xFFFF;

pub const BIG_SIZE: u16 = 20;
pub const FIRST_FREE_CHUNK_LIST: u16 = 0;

// FIRST_FREE_CHUNK_LIST + BIG_SIZE
pub const LAST_BIG_CHUNK_LIST: u16 = 20;

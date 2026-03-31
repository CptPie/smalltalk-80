//! This file contains the implementation of the object pointer (OOP) type and its related functions

use crate::errors::ObjectMemoryError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OOP {
    value: u16,
}

impl OOP {
    /// Create a new OOP object from a raw memory value, no verification.
    pub fn from_raw(value: u16) -> OOP {
        OOP { value }
    }

    /// Checks if an object pointer points to an object or an integer value
    /// returns true if it is an object pointer
    /// returns false if it is an integer value
    ///
    /// see Bluebook p. 660
    ///
    /// isIntegerObject: objectPointer
    ///   ↑(objectPointer bitAnd: 1) = 1
    ///
    pub fn is_integer_object(&self) -> bool {
        return (self.value & 0x1) == 1;
    }

    /// Validates an OOP to be an object.
    /// returns the object value if it is an object (unmodified so the low bit is always 0)
    ///      This aspect is not part of the specification but makes more sense in my opinion
    /// returns ObjectMemoryError::InvalidSmallIntegerAccess errors if the OOP points to an integer
    ///
    /// see Bluebook p. 661
    ///
    /// cantBeIntegerObject: objectPointer
    ///   (self isIntegerObject: objectPointer)
    ///     ifTrue: [Sensor notify: `A small integer has no object table entry`]
    ///
    fn cant_be_integer_object(&self) -> Result<u16, ObjectMemoryError> {
        if self.is_integer_object() {
            return Err(ObjectMemoryError::InvalidSmallIntegerAccess);
        } else {
            return Ok(self.value);
        }
    }

    /// Access the integer value of the OOP
    /// returns the integer value if OOP represents an integer
    /// returns ObjectMemoryError::NotInteger if it represents an object pointer
    ///
    /// see Bluebook p. 688
    ///
    /// integerValueOf: objectPointer
    ///   ↑objectPointer/2
    ///
    pub fn integer_value_of(&self) -> Result<i16, ObjectMemoryError> {
        if self.is_integer_object() {
            return Ok((self.value as i16) >> 1);
        } else {
            return Err(ObjectMemoryError::NotInteger);
        }
    }

    /// Verifies the requested value to be within the SmallInteger range
    /// (-2^15 .. 2^15-1)
    /// returns true if the value is in the correct range
    /// returns false if it falls outside
    ///
    /// see Bluebook p. 688 (note that there are typos and logic errors)
    ///
    /// integerValue: valueWord
    ///   ↑valueWord <= -16384 and: [valueWord > 16834]
    ///
    pub fn is_integer_value(value: i16) -> bool {
        return value >= -16384 && value < 16383;
    }

    /// Converts a integer value to an OOP object
    ///
    /// returns the OOP object if the value could be converted
    /// returns ObjectMemoryError::NotInIntegerRange error otherwise
    ///
    /// see Bluebook p. 688
    ///
    /// integerObjectOf: value
    ///   ↑(value bitShift: 1) + 1
    ///
    pub fn integer_object_of(value: i16) -> Result<OOP, ObjectMemoryError> {
        if Self::is_integer_value(value) {
            return Ok(OOP {
                value: ((value << 1) | 1) as u16,
            });
        } else {
            return Err(ObjectMemoryError::NotInIntegerRange);
        }
    }
}

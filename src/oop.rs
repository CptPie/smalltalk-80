//! This file contains the implementation of the object pointer (OOP) type and its related functions

use crate::errors::ObjectMemoryError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OOP {
    pub value: u16,
}

impl OOP {
    /// Create a new OOP object from a raw memory value, no verification.
    ///
    /// # Parameters:
    ///     * value (u16): The raw 16-bit value to wrap
    ///
    /// # Returns:
    ///     * OOP, the new object pointer
    pub fn from_raw(value: u16) -> OOP {
        OOP { value }
    }

    /// Checks if an object pointer points to an object or an integer value.
    ///
    /// # Returns:
    ///     * bool, true if the OOP encodes an integer, false if it is an object pointer
    //
    //  see Bluebook p. 660
    //
    //  isIntegerObject: objectPointer
    //    ↑(objectPointer bitAnd: 1) = 1
    //
    pub fn is_integer_object(&self) -> bool {
        return (self.value & 0x1) == 1;
    }

    /// Validates an OOP to be an object, not a SmallInteger.
    ///
    /// # Returns:
    ///     * Result<u16, ObjectMemoryError>, the object value if it is an object (unmodified
    ///       so the low bit is always 0), or InvalidSmallIntegerAccess if it is an integer
    ///       (Note: Returning the Object is not part of the specification but makes more sense
    ///       in my opinion.)
    //
    //  see Bluebook p. 661
    //
    //  cantBeIntegerObject: objectPointer
    //    (self isIntegerObject: objectPointer)
    //      ifTrue: [Sensor notify: `A small integer has no object table entry`]
    //
    fn cant_be_integer_object(&self) -> Result<u16, ObjectMemoryError> {
        if self.is_integer_object() {
            return Err(ObjectMemoryError::InvalidSmallIntegerAccess);
        } else {
            return Ok(self.value);
        }
    }

    /// Access the integer value of the OOP.
    ///
    /// # Returns:
    ///     * Result<i16, ObjectMemoryError>, the integer value if OOP represents an integer,
    ///       or NotInteger if it represents an object pointer
    //
    //  see Bluebook p. 688
    //
    //  integerValueOf: objectPointer
    //    ↑objectPointer/2
    //
    pub fn integer_value_of(&self) -> Result<i16, ObjectMemoryError> {
        if self.is_integer_object() {
            return Ok((self.value as i16) >> 1);
        } else {
            return Err(ObjectMemoryError::NotInteger);
        }
    }

    /// Verifies the requested value to be within the SmallInteger range (-16384..16383).
    ///
    /// # Parameters:
    ///     * value (i16): The value to check
    ///
    /// # Returns:
    ///     * bool, true if the value is in the correct range, false if it falls outside
    //
    //  see Bluebook p. 688 (note that there are typos and logic errors)
    //
    //  integerValue: valueWord
    //    ↑valueWord <= -16384 and: [valueWord > 16834]
    //
    pub fn is_integer_value(value: i16) -> bool {
        return value >= -16384 && value < 16384;
    }

    /// Converts an integer value to an OOP object.
    ///
    /// # Parameters:
    ///     * value (i16): The integer value to encode
    ///
    /// # Returns:
    ///     * Result<OOP, ObjectMemoryError>, the encoded OOP if the value is in range,
    ///       or NotInIntegerRange otherwise
    //
    //  see Bluebook p. 688
    //
    //  integerObjectOf: value
    //    ↑(value bitShift: 1) + 1
    //
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_raw_creates_oop() {
        let val = 0x02; // 0x02 is the NilPointer 'constant'
        let oop = OOP::from_raw(val);
        assert!(!oop.is_integer_object()); // should be an object
        assert_eq!(oop.cant_be_integer_object(), Ok(val));
        assert!(!oop.cant_be_integer_object().is_err());
    }

    #[test]
    fn is_integer_object_truthy() {
        let val = 0x3; // SmallInteger of value '1'
        let oop = OOP::from_raw(val);
        assert!(oop.is_integer_object());
        assert_eq!(oop.integer_value_of(), Ok(0x01));
    }

    #[test]
    fn is_integer_object_falsy() {
        let val = 0x4; // An object value 
        let oop = OOP::from_raw(val);
        assert!(!oop.is_integer_object());
    }

    #[test]
    fn cant_be_integer_object_errors_on_integer() {
        let val = 0x3; // SmallInteger of value '1'
        let oop = OOP::from_raw(val);
        assert!(oop.cant_be_integer_object().is_err());
    }

    #[test]
    fn cant_be_integer_object_returns_value_on_object() {
        let val = 0x4;
        let oop = OOP::from_raw(val);
        assert_eq!(oop.cant_be_integer_object(), Ok(val));
    }

    #[test]
    fn integer_value_of_returns_integer() {
        let val = 0x3;
        let oop = OOP::from_raw(val);
        assert_eq!(oop.integer_value_of(), Ok(0x01));
    }

    #[test]
    fn integer_value_of_returns_error() {
        let val = 0x02;
        let oop = OOP::from_raw(val);
        assert!(oop.integer_value_of().is_err());
    }

    #[test]
    fn is_integer_value_truthy() {
        // some values within the range
        let val1 = 12345;
        assert!(OOP::is_integer_value(val1));
        let val2 = -12345;
        assert!(OOP::is_integer_value(val2));
        // zero
        let val3 = 0;
        assert!(OOP::is_integer_value(val3));
        // lower bound
        let val4 = -16384;
        assert!(OOP::is_integer_value(val4));
        // upper bound
        let val5 = 16383;
        assert!(OOP::is_integer_value(val5));
    }

    #[test]
    fn is_integer_value_falsy_low() {
        let val = -16385;
        assert!(!OOP::is_integer_value(val));
    }

    #[test]
    fn is_integer_value_falsy_high() {
        let val = 16834;
        assert!(!OOP::is_integer_value(val));
    }

    #[test]
    fn integer_object_of_returns_integer_oop() {
        let val1 = 12345;
        assert_eq!(OOP::integer_object_of(val1), Ok(OOP::from_raw(0x6073)));
        let val2 = -12345;
        assert_eq!(OOP::integer_object_of(val2), Ok(OOP::from_raw(0x9f8f)));
    }

    #[test]
    fn integer_object_of_returns_error() {
        let val = 20000;
        assert!(OOP::integer_object_of(val).is_err())
    }
}

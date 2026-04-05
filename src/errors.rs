use std::fmt;

#[derive(Debug, PartialEq)]
pub enum ObjectMemoryError {
    InvalidSmallIntegerAccess,
    NotInteger,
    NotInIntegerRange,
    NoFreeEntries,
}

impl fmt::Display for ObjectMemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSmallIntegerAccess => {
                write!(f, "A small integer has no object table entry")
            }
            Self::NotInteger => write!(f, "Object pointer is not a SmallInteger"),
            Self::NotInIntegerRange => write!(
                f,
                "Value is outside of the SmallInteger range (-2^15..2^15-1)"
            ),
            Self::NoFreeEntries => write!(f, "No free entries remaining in free pointer list"),
        }
    }
}

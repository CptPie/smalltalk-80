use crate::oop::OOP;

type HeapSegment = Vec<u16>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMemory {
    heap: Vec<HeapSegment>,
    object_table: Vec<u16>,
}

// ====================================
// Functions for ObjectTableEntries
// ====================================

impl ObjectMemory {
    // Structure of an ObjectTableEntry:
    //  Word 0:
    //     ┌────────┬───┬───┬───┬───┬─────────┐
    //     │ COUNT  │ O │ P │ F │ ? │ SEGMENT │
    //     │ (8 bit)│(1)│(1)│(1)│(1)│ (4 bit) │
    //     └────────┴───┴───┴───┴───┴─────────┘
    // bits:  15-8    7   6   5   4    3-0
    //
    //  Word 1:
    //     ┌──────────────────────────────────┐
    //     │             LOCATION             │  ← 16-bit word address within segment
    //     └──────────────────────────────────┘
    // bits:               15-0
    //
    // ┌─────────────────────┐
    // │       GETTERS       │
    // └─────────────────────┘

    fn count_bits_of(&self, oop: OOP) -> u8 {
        let word0 = self.object_table[oop.value as usize];
        return (word0 >> 8) as u8;
    }

    fn odd_bit_of(&self, oop: OOP) -> bool {
        let word0 = self.object_table[oop.value as usize];
        return (word0 >> 7) & 1 == 1;
    }

    fn pointer_bit_of(&self, oop: OOP) -> bool {
        let word0 = self.object_table[oop.value as usize];
        return (word0 >> 6) & 1 == 1;
    }

    fn free_bit_of(&self, oop: OOP) -> bool {
        let word0 = self.object_table[oop.value as usize];
        return (word0 >> 5) & 1 == 1;
    }

    fn segment_bits_of(&self, oop: OOP) -> u8 {
        let word0 = self.object_table[oop.value as usize];
        return (word0 & 0xF) as u8;
    }

    fn location_bits_of(&self, oop: OOP) -> u16 {
        return self.object_table[(oop.value + 1) as usize];
    }

    // ┌─────────────────────┐
    // │       SETTERS       │
    // └─────────────────────┘

    fn count_bits_of_put(&mut self, oop: OOP, new_count: u8) {
        let mut word0 = self.object_table[oop.value as usize];
        word0 = (word0 & 0x00FF) | ((new_count as u16) << 8); // clear old count, set new
        self.object_table[oop.value as usize] = word0
    }

    fn odd_bit_of_put(&mut self, oop: OOP, new_bit: bool) {
        let mut word0 = self.object_table[oop.value as usize];
        word0 = word0 & 0xFF7F;
        if new_bit {
            word0 = word0 | 0x0080;
        }
        self.object_table[oop.value as usize] = word0
    }

    fn pointer_bit_of_put(&mut self, oop: OOP, new_bit: bool) {
        let mut word0 = self.object_table[oop.value as usize];
        word0 = word0 & 0xFFBF;
        if new_bit {
            word0 = word0 | 0x0040;
        }
        self.object_table[oop.value as usize] = word0
    }

    fn free_bit_of_put(&mut self, oop: OOP, new_bit: bool) {
        let mut word0 = self.object_table[oop.value as usize];
        word0 = word0 & 0xFFDF;
        if new_bit {
            word0 = word0 | 0x0020;
        }
        self.object_table[oop.value as usize] = word0
    }

    fn segment_bits_of_put(&mut self, oop: OOP, new_segment: u8) {
        let mut word0 = self.object_table[oop.value as usize];
        word0 = (word0 & 0xFFF0) | (new_segment as u16) & 0x000F;
        self.object_table[oop.value as usize] = word0
    }

    fn location_bits_of_put(&mut self, oop: OOP, new_location: u16) {
        self.object_table[(oop.value + 1) as usize] = new_location
    }
}

#[cfg(test)]
mod ot_accessor_tests {
    use super::*;

    fn dummy_memory() -> ObjectMemory {
        return ObjectMemory {
            heap: vec![vec![0u16; 64]],
            object_table: vec![0u16; 64],
        };
    }

    #[test]
    fn count_bits_of_returns_correct_values() {
        let mut memory = dummy_memory();
        memory.object_table[0] = 0xAB00;
        assert_eq!(memory.count_bits_of(OOP::from_raw(0)), 0xAB)
    }

    #[test]
    fn count_bits_of_put_sets_correct_values() {
        let mut memory = dummy_memory();
        memory.object_table[0] = 0xAB00;
        memory.object_table[2] = 0x0000;
        memory.count_bits_of_put(OOP::from_raw(0), 0x00);
        memory.count_bits_of_put(OOP::from_raw(2), 0xAB);
        assert_eq!(memory.object_table[0], 0x0000);
        assert_eq!(memory.object_table[1], 0x0000);
        assert_eq!(memory.object_table[2], 0xAB00);
        assert_eq!(memory.object_table[3], 0x0000);
    }

    #[test]
    fn odd_bit_of_returns_correct_value() {
        let mut memory = dummy_memory();
        memory.object_table[0] = 0xAB80;
        memory.object_table[2] = 0xAB00;
        assert!(memory.odd_bit_of(OOP::from_raw(0)));
        assert!(!memory.odd_bit_of(OOP::from_raw(2)));
    }

    #[test]
    fn odd_bit_of_put_sets_correct_value() {
        let mut memory = dummy_memory();
        memory.object_table[0] = 0xAB80;
        memory.odd_bit_of_put(OOP::from_raw(0), false);
        memory.object_table[2] = 0xAB00;
        memory.odd_bit_of_put(OOP::from_raw(2), true);
        assert_eq!(memory.object_table[0], 0xAB00);
        assert_eq!(memory.object_table[1], 0x0000);
        assert_eq!(memory.object_table[2], 0xAB80);
        assert_eq!(memory.object_table[3], 0x0000);
    }

    #[test]
    fn pointer_bit_of_returns_correct_value() {
        let mut memory = dummy_memory();
        memory.object_table[0] = 0xAB40;
        memory.object_table[2] = 0xAB00;
        assert!(memory.pointer_bit_of(OOP::from_raw(0)));
        assert!(!memory.pointer_bit_of(OOP::from_raw(2)));
    }

    #[test]
    fn pointer_bit_of_put_sets_correct_value() {
        let mut memory = dummy_memory();
        memory.object_table[0] = 0xAB40;
        memory.pointer_bit_of_put(OOP::from_raw(0), false);
        memory.object_table[2] = 0xAB00;
        memory.pointer_bit_of_put(OOP::from_raw(2), true);
        assert_eq!(memory.object_table[0], 0xAB00);
        assert_eq!(memory.object_table[1], 0x0000);
        assert_eq!(memory.object_table[2], 0xAB40);
        assert_eq!(memory.object_table[3], 0x0000);
    }

    #[test]
    fn free_bit_of_returns_correct_value() {
        let mut memory = dummy_memory();
        memory.object_table[0] = 0xAB20;
        memory.object_table[2] = 0xAB00;
        assert!(memory.free_bit_of(OOP::from_raw(0)));
        assert!(!memory.free_bit_of(OOP::from_raw(2)));
    }

    #[test]
    fn free_bit_of_put_sets_correct_value() {
        let mut memory = dummy_memory();
        memory.object_table[0] = 0xAB20;
        memory.free_bit_of_put(OOP::from_raw(0), false);
        memory.object_table[2] = 0xAB00;
        memory.free_bit_of_put(OOP::from_raw(2), true);
        assert_eq!(memory.object_table[0], 0xAB00);
        assert_eq!(memory.object_table[1], 0x0000);
        assert_eq!(memory.object_table[2], 0xAB20);
        assert_eq!(memory.object_table[3], 0x0000);
    }

    #[test]
    fn segment_bit_of_returns_correct_value() {
        let mut memory = dummy_memory();
        memory.object_table[0] = 0xAB0F;
        memory.object_table[2] = 0xAB03;
        assert_eq!(memory.segment_bits_of(OOP::from_raw(0)), 0xF);
        assert_eq!(memory.segment_bits_of(OOP::from_raw(2)), 0x3);
    }

    #[test]
    fn segment_bits_of_put_sets_correct_value() {
        let mut memory = dummy_memory();
        memory.object_table[0] = 0xABCD;
        memory.object_table[1] = 0xDEAD;
        memory.segment_bits_of_put(OOP::from_raw(0), 0xEF);
        memory.object_table[2] = 0xEFAB;
        memory.object_table[3] = 0xBEEF;
        memory.segment_bits_of_put(OOP::from_raw(2), 0xCD);
        assert_eq!(memory.object_table[0], 0xABCF);
        assert_eq!(memory.object_table[1], 0xDEAD);
        assert_eq!(memory.object_table[2], 0xEFAD);
        assert_eq!(memory.object_table[3], 0xBEEF);
    }

    #[test]
    fn location_bit_of_returns_correct_value() {
        let mut memory = dummy_memory();
        memory.object_table[0] = 0xDEAD;
        memory.object_table[1] = 0xBEEF;
        memory.object_table[2] = 0xC0FF;
        memory.object_table[3] = 0xBABE;
        assert_eq!(memory.location_bits_of(OOP::from_raw(0)), 0xBEEF);
        assert_eq!(memory.location_bits_of(OOP::from_raw(2)), 0xBABE);
    }

    #[test]
    fn location_bits_of_put_sets_correct_value() {
        let mut memory = dummy_memory();
        memory.object_table[0] = 0xDEAD;
        memory.object_table[1] = 0xBEEF;
        memory.object_table[2] = 0xC0FF;
        memory.object_table[3] = 0xBABE;
        memory.location_bits_of_put(OOP::from_raw(0), 0xABCD);
        memory.location_bits_of_put(OOP::from_raw(2), 0xEFAB);
        assert_eq!(memory.object_table[0], 0xDEAD);
        assert_eq!(memory.object_table[1], 0xABCD);
        assert_eq!(memory.object_table[2], 0xC0FF);
        assert_eq!(memory.object_table[3], 0xEFAB);
    }
}

// ====================================
//  Heap Access
// ====================================

impl ObjectMemory {
    // we work with heap memory here, so we have to resolve the object pointers
    // to find out the location of the object

    fn heap_chunk_of_word(&self, oop: OOP, word_index: u16) -> u16 {
        let seg = self.segment_bits_of(oop) as usize;
        let loc = self.location_bits_of(oop) as usize;
        return self.heap[seg][loc + word_index as usize];
    }
    fn heap_chunk_of_word_put(&mut self, oop: OOP, word_index: u16, value: u16) {
        let seg = self.segment_bits_of(oop) as usize;
        let loc = self.location_bits_of(oop) as usize;
        self.heap[seg][loc + word_index as usize] = value;
    }

    fn heap_chunk_of_byte(&self, oop: OOP, byte_index: u16) -> u8 {
        let seg = self.segment_bits_of(oop) as usize;
        let loc = self.location_bits_of(oop) as usize;
        let word = self.heap[seg][loc + (byte_index / 2) as usize];
        // Big Endian.
        if byte_index % 2 == 0 {
            // Even byte_index is the upper byte
            return (word >> 8) as u8;
        } else {
            // Odd byte_index is the lower byte, so we drop the high byte
            return (word & 0x00FF) as u8;
        }
    }

    fn heap_chunk_of_byte_put(&mut self, oop: OOP, byte_index: u16, value: u8) {
        let seg = self.segment_bits_of(oop) as usize;
        let loc = self.location_bits_of(oop) as usize;
        let mut word = self.heap[seg][loc + (byte_index / 2) as usize];
        // Big Endian.
        if byte_index % 2 == 0 {
            // Even byte_index is the upper byte
            word = word & 0x00FF;
            word = word | ((value as u16) << 8);
        } else {
            // Odd byte_index is the lower byte
            word = word & 0xFF00;
            word = word | value as u16;
        }
        self.heap[seg][loc + (byte_index / 2) as usize] = word;
    }

    fn size_bits_of(&self, oop: OOP) -> u16 {
        return self.heap_chunk_of_word(oop, 0);
    }

    fn size_bits_of_put(&mut self, oop: OOP, value: u16) {
        self.heap_chunk_of_word_put(oop, 0, value);
    }

    fn class_bits_of(&self, oop: OOP) -> u16 {
        return self.heap_chunk_of_word(oop, 1);
    }

    fn class_bits_of_put(&mut self, oop: OOP, value: u16) {
        self.heap_chunk_of_word_put(oop, 1, value);
    }
}

#[cfg(test)]
mod heap_accessor_tests {
    use super::*;

    fn dummy_memory() -> ObjectMemory {
        let mut mem = ObjectMemory {
            heap: vec![vec![0u16; 64]],
            object_table: vec![0u16; 64],
        };
        // OOP 0: segment 0, location 10
        mem.object_table[0] = 0x0000;
        mem.object_table[1] = 0x000A;

        // Heap data starting at 10
        mem.heap[0][10] = 0x0006; //size 6 words
        mem.heap[0][11] = 0x0020; // class pointer
        mem.heap[0][12] = 0xAAAA; // field 1
        mem.heap[0][13] = 0xBBBB; // field 2
        mem.heap[0][14] = 0xCCCC; // ...
        mem.heap[0][15] = 0xDDDD; // field {size} -2 (6-2=4)

        return mem;
    }

    #[test]
    fn heap_chunk_of_word_returns_correct_value() {
        let memory = dummy_memory();
        assert_eq!(memory.heap_chunk_of_word(OOP::from_raw(0), 0), 0x0006);
        assert_eq!(memory.heap_chunk_of_word(OOP::from_raw(0), 1), 0x0020);
        assert_eq!(memory.heap_chunk_of_word(OOP::from_raw(0), 2), 0xAAAA);
        assert_eq!(memory.heap_chunk_of_word(OOP::from_raw(0), 5), 0xDDDD);
    }

    #[test]
    fn heap_chunk_of_word_put_sets_correct_value() {
        let mut memory = dummy_memory();
        memory.heap_chunk_of_word_put(OOP::from_raw(0), 0, 5);
        assert_eq!(memory.heap[0][10], 0x0005);
        assert_eq!(memory.heap[0][11], 0x0020);
        assert_eq!(memory.heap[0][12], 0xAAAA);
        memory.heap_chunk_of_word_put(OOP::from_raw(0), 3, 0xBEEF);
        assert_eq!(memory.heap[0][13], 0xBEEF);
        assert_ne!(memory.heap[0][13], 0xBBBB);
    }

    #[test]
    fn heap_chunk_of_byte_returns_correct_value() {
        let memory = dummy_memory();
        assert_eq!(memory.heap_chunk_of_byte(OOP::from_raw(0), 0), 0x00);
        assert_eq!(memory.heap_chunk_of_byte(OOP::from_raw(0), 1), 0x06);
        assert_eq!(memory.heap_chunk_of_byte(OOP::from_raw(0), 7), 0xBB);
        assert_eq!(memory.heap_chunk_of_byte(OOP::from_raw(0), 8), 0xCC);
    }

    #[test]
    fn heap_chunk_of_byte_put_sets_correct_value() {
        let mut memory = dummy_memory();
        memory.heap_chunk_of_byte_put(OOP::from_raw(0), 1, 0x05);
        memory.heap_chunk_of_byte_put(OOP::from_raw(0), 7, 0xEE);
        memory.heap_chunk_of_byte_put(OOP::from_raw(0), 8, 0xFF);
        assert_eq!(memory.heap[0][10], 0x0005);
        assert_eq!(memory.heap[0][13], 0xBBEE);
        assert_eq!(memory.heap[0][14], 0xFFCC);
    }

    #[test]
    fn size_bits_of_returns_correct_value() {
        let memory = dummy_memory();
        assert_eq!(memory.size_bits_of(OOP::from_raw(0)), 0x0006);
    }

    #[test]
    fn size_bits_of_put_sets_correct_value() {
        let mut memory = dummy_memory();
        memory.size_bits_of_put(OOP::from_raw(0), 0xABCD);
        assert_eq!(memory.heap[0][10], 0xABCD);
    }

    #[test]
    fn class_bits_of_returns_correct_value() {
        let memory = dummy_memory();
        assert_eq!(memory.class_bits_of(OOP::from_raw(0)), 0x0020);
    }

    #[test]
    fn class_bits_of_put_sets_correct_value() {
        let mut memory = dummy_memory();
        memory.class_bits_of_put(OOP::from_raw(0), 0xABCD);
        assert_eq!(memory.heap[0][11], 0xABCD);
    }
}

// ====================================
//  Public facing API
// ====================================

impl ObjectMemory {}

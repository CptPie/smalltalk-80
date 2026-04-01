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
    //
    //
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
        word0 = (word0 & 0xFFF0) | (new_segment as u16);
        self.object_table[oop.value as usize] = word0
    }

    fn location_bits_of_put(&mut self, oop: OOP, new_location: u16) {
        self.object_table[(oop.value + 1) as usize] = new_location
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

use std::{
    cmp::min,
    ops::{Index, IndexMut},
};

use crate::{
    errors::ObjectMemoryError,
    globalconstants::{
        BIG_SIZE, CLASS_SMALL_INTEGER_POINTER, FIRST_FREE_CHUNK_LIST, HEADER_SIZE,
        HEAP_SEGMENT_SIZE, HEAP_SIZE, LAST_BIG_CHUNK_LIST, NIL_POINTER, NON_POINTER,
    },
    oop::OOP,
};

// Custom Type definitions
#[derive(Clone, Debug, PartialEq, Eq)]
struct HeapSegment {
    data: Vec<u16>,
}
impl HeapSegment {
    fn new() -> Self {
        HeapSegment {
            data: vec![0u16; HEAP_SEGMENT_SIZE],
        }
    }
}

// indexing overwrites to make access as easy as array access
impl Index<usize> for HeapSegment {
    type Output = u16;

    fn index(&self, index: usize) -> &u16 {
        &self.data[index]
    }
}

impl IndexMut<usize> for HeapSegment {
    fn index_mut(&mut self, index: usize) -> &mut u16 {
        &mut self.data[index]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Heap {
    data: Vec<HeapSegment>,
}

impl Heap {
    fn new() -> Self {
        Heap {
            data: vec![HeapSegment::new(); HEAP_SIZE],
        }
    }
}

// indexing overwrites to make access as easy as array access
impl Index<usize> for Heap {
    type Output = HeapSegment;

    fn index(&self, index: usize) -> &HeapSegment {
        &self.data[index]
    }
}

impl IndexMut<usize> for Heap {
    fn index_mut(&mut self, index: usize) -> &mut HeapSegment {
        &mut self.data[index]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMemory {
    heap: Heap,
    object_table: Vec<u16>,
    free_pointer_list: u16,
    current_segment: u8,
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

    fn to_free_pointer_list_add(&mut self, oop: OOP) {
        self.free_bit_of_put(oop, true);
        self.location_bits_of_put(oop, self.free_pointer_list);
        self.free_pointer_list = oop.value;
    }

    fn remove_from_free_pointer_list(&mut self) -> Result<OOP, ObjectMemoryError> {
        let head = self.free_pointer_list;
        if head == NON_POINTER {
            return Err(ObjectMemoryError::NoFreeEntries);
        }
        let oop = OOP::from_raw(head);
        self.free_pointer_list = self.location_bits_of(oop);
        self.free_bit_of_put(oop, false);
        return Ok(oop);
    }
}

#[cfg(test)]
mod ot_accessor_tests {
    use crate::globalconstants::NON_POINTER;

    use super::*;

    fn dummy_memory() -> ObjectMemory {
        return ObjectMemory {
            heap: Heap::new(),
            object_table: vec![0u16; 64],
            free_pointer_list: NON_POINTER,
            current_segment: 0,
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

#[cfg(test)]
mod ot_free_list_tests {
    use crate::globalconstants::NON_POINTER;

    use super::*;

    fn dummy_memory() -> ObjectMemory {
        return ObjectMemory {
            heap: Heap::new(),
            object_table: vec![0u16; 64],
            free_pointer_list: NON_POINTER,
            current_segment: 0,
        };
    }

    #[test]
    fn add_to_empty_list() {
        let mut memory = dummy_memory();
        memory.to_free_pointer_list_add(OOP::from_raw(4));
        assert_eq!(memory.free_pointer_list, 4);
        assert!(memory.free_bit_of(OOP::from_raw(4)));
        assert_eq!(memory.location_bits_of(OOP::from_raw(4)), NON_POINTER);
    }

    #[test]
    fn add_multiple_to_list() {
        let mut memory = dummy_memory();
        memory.to_free_pointer_list_add(OOP::from_raw(4));
        memory.to_free_pointer_list_add(OOP::from_raw(6));
        memory.to_free_pointer_list_add(OOP::from_raw(8));
        // head should be the last added
        assert_eq!(memory.free_pointer_list, 8);
        // chain: 8 -> 6 -> 4 -> NON_POINTER
        assert_eq!(memory.location_bits_of(OOP::from_raw(8)), 6);
        assert_eq!(memory.location_bits_of(OOP::from_raw(6)), 4);
        assert_eq!(memory.location_bits_of(OOP::from_raw(4)), NON_POINTER);
    }

    #[test]
    fn remove_from_list() {
        let mut memory = dummy_memory();
        memory.to_free_pointer_list_add(OOP::from_raw(4));
        memory.to_free_pointer_list_add(OOP::from_raw(6));
        let result = memory.remove_from_free_pointer_list();
        assert_eq!(result, Ok(OOP::from_raw(6)));
        assert!(!memory.free_bit_of(OOP::from_raw(6)));
        assert_eq!(memory.free_pointer_list, 4);
    }

    #[test]
    fn remove_until_empty() {
        let mut memory = dummy_memory();
        memory.to_free_pointer_list_add(OOP::from_raw(4));
        memory.to_free_pointer_list_add(OOP::from_raw(6));
        assert!(memory.remove_from_free_pointer_list().is_ok());
        assert!(memory.remove_from_free_pointer_list().is_ok());
        assert!(memory.remove_from_free_pointer_list().is_err());
    }

    #[test]
    fn remove_from_empty_list_returns_error() {
        let mut memory = dummy_memory();
        assert!(memory.remove_from_free_pointer_list().is_err());
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

    // The heap is segmented into 'lists' of u16 words. Each segment holds its own Free Lists.
    // The Lists start at FIRST_FREE_CHUNK_LIST at size 2 (chunk header size), the second at size 3
    // and so on till BIG_SIZE=20 which is the 'overflow' list for everything bigger than 20 words.
    // The head of a list points at the most recently freed chunk of that size.
    // When we want to add a new item to the free list, we set that chunks class field (used here
    // to point to the previous head of the list) to the old head. The size of the new chunk freed
    // chunk is written to word 0 (as a double reference which list it is a part of [remember lists
    // are determined by size]).

    fn to_free_chunk_list_add(&mut self, segment: u8, size: u16, chunk_location: u16) {
        // determine which list to insert into
        let list_index = min(size, BIG_SIZE);
        // get current head
        let list_head = self.heap[segment as usize][(FIRST_FREE_CHUNK_LIST + list_index) as usize];
        // write header at the freed chunk
        self.heap[segment as usize][chunk_location as usize] = size;
        self.heap[segment as usize][(chunk_location + 1) as usize] = list_head;
        // point the head to the new chunk
        self.heap[segment as usize][(FIRST_FREE_CHUNK_LIST + list_index) as usize] = chunk_location;
    }

    fn remove_from_free_chunk_list(
        &mut self,
        segment: u8,
        size: u16,
    ) -> Result<u16, ObjectMemoryError> {
        // determine which list to remove from
        let list_index = min(size, BIG_SIZE);
        // get current head
        let target_chunk =
            self.heap[segment as usize][(FIRST_FREE_CHUNK_LIST + list_index) as usize];
        if target_chunk == NON_POINTER {
            return Err(ObjectMemoryError::NoFreeEntries);
        }
        // advance to next chunk
        let next_chunk = self.heap[segment as usize][(target_chunk + 1) as usize];
        self.heap[segment as usize][(FIRST_FREE_CHUNK_LIST + list_index) as usize] = next_chunk;
        return Ok(target_chunk);
    }
}

#[cfg(test)]
mod heap_accessor_tests {
    use crate::globalconstants::NON_POINTER;

    use super::*;

    fn dummy_memory() -> ObjectMemory {
        let mut mem = ObjectMemory {
            heap: Heap::new(),
            object_table: vec![0u16; 64],
            free_pointer_list: NON_POINTER,
            current_segment: 0,
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

#[cfg(test)]
mod heap_free_chunk_tests {
    use crate::globalconstants::{BIG_SIZE, FIRST_FREE_CHUNK_LIST, NON_POINTER};

    use super::*;

    fn dummy_memory() -> ObjectMemory {
        let mut mem = ObjectMemory {
            heap: Heap::new(),
            object_table: vec![0u16; 64],
            free_pointer_list: NON_POINTER,
            current_segment: 0,
        };
        // initialize free chunk list heads to NON_POINTER (empty)
        for i in 0..=(BIG_SIZE as usize) {
            mem.heap[0][(FIRST_FREE_CHUNK_LIST as usize) + i] = NON_POINTER;
        }
        return mem;
    }

    #[test]
    fn add_chunk_to_empty_list() {
        let mut memory = dummy_memory();
        memory.to_free_chunk_list_add(0, 4, 30);
        // list head for size 4 should point to chunk at 30
        assert_eq!(memory.heap[0][(FIRST_FREE_CHUNK_LIST + 4) as usize], 30);
        // chunk's size field
        assert_eq!(memory.heap[0][30], 4);
        // chunk's next pointer should be NON_POINTER (was empty)
        assert_eq!(memory.heap[0][31], NON_POINTER);
    }

    #[test]
    fn add_multiple_chunks_to_same_list() {
        let mut memory = dummy_memory();
        memory.to_free_chunk_list_add(0, 4, 30);
        memory.to_free_chunk_list_add(0, 4, 40);
        // head should point to most recently added
        assert_eq!(memory.heap[0][(FIRST_FREE_CHUNK_LIST + 4) as usize], 40);
        // chain: 40 -> 30 -> NON_POINTER
        assert_eq!(memory.heap[0][41], 30);
        assert_eq!(memory.heap[0][31], NON_POINTER);
    }

    #[test]
    fn add_big_chunk_goes_to_big_list() {
        let mut memory = dummy_memory();
        memory.to_free_chunk_list_add(0, 25, 30);
        // should go to BIG_SIZE list, not size 25
        assert_eq!(
            memory.heap[0][(FIRST_FREE_CHUNK_LIST + BIG_SIZE) as usize],
            30
        );
        // but size field should still be 25 (actual size)
        assert_eq!(memory.heap[0][30], 25);
    }

    #[test]
    fn remove_chunk_from_list() {
        let mut memory = dummy_memory();
        memory.to_free_chunk_list_add(0, 4, 30);
        memory.to_free_chunk_list_add(0, 4, 40);
        let result = memory.remove_from_free_chunk_list(0, 4);
        assert_eq!(result, Ok(40));
        // head should now point to 30
        assert_eq!(memory.heap[0][(FIRST_FREE_CHUNK_LIST + 4) as usize], 30);
    }

    #[test]
    fn remove_until_empty() {
        let mut memory = dummy_memory();
        memory.to_free_chunk_list_add(0, 4, 30);
        memory.to_free_chunk_list_add(0, 4, 40);
        assert_eq!(memory.remove_from_free_chunk_list(0, 4), Ok(40));
        assert_eq!(memory.remove_from_free_chunk_list(0, 4), Ok(30));
        assert!(memory.remove_from_free_chunk_list(0, 4).is_err());
    }

    #[test]
    fn remove_from_empty_list_returns_error() {
        let mut memory = dummy_memory();
        assert!(memory.remove_from_free_chunk_list(0, 4).is_err());
    }

    #[test]
    fn different_sizes_use_different_lists() {
        let mut memory = dummy_memory();
        memory.to_free_chunk_list_add(0, 4, 30);
        memory.to_free_chunk_list_add(0, 6, 40);
        // removing size 4 should give 30, not 40
        assert_eq!(memory.remove_from_free_chunk_list(0, 4), Ok(30));
        // removing size 6 should give 40
        assert_eq!(memory.remove_from_free_chunk_list(0, 6), Ok(40));
    }
}

// ====================================
//  Public facing API
// ====================================

impl ObjectMemory {
    // ┌─────────────────────┐
    // │   POINTER ACCESS    │
    // └─────────────────────┘

    /// Fetch a specific field of an object.
    ///
    /// Parameters:
    ///     - pointer: The pointer to the object
    ///     - field_index: The 0 indexed field that shall be fetched
    ///
    /// Returns:
    ///     - u16, the raw field data
    pub fn fetch_pointer(&self, field_index: u16, pointer: OOP) -> u16 {
        return self.heap_chunk_of_word(pointer, HEADER_SIZE + field_index);
    }

    /// Store a value to a specific field of an object.
    ///
    /// This also handles modifying all reference counts.
    ///
    /// Parameters:
    ///     - pointer: The pointer to the object
    ///     - field_index: The 0 indexed field that the value shall be stored at
    ///     - value: the value to be stored
    pub fn store_pointer(&mut self, field_index: u16, pointer: OOP, value: u16) {
        let old_value = self.fetch_pointer(field_index, pointer);
        self.increase_references_to(OOP::from_raw(value));
        self.decrease_references_to(OOP::from_raw(old_value));
        self.heap_chunk_of_word_put(pointer, HEADER_SIZE + field_index, value);
    }

    // ┌─────────────────────┐
    // │     RAW ACCESS      │
    // └─────────────────────┘

    /// Fetch a specific word of an object, without accounting for the header offset.
    ///
    /// Parameters:
    ///     - pointer: The pointer to the object
    ///     - word_index: The 0 indexed word that shall be fetched
    ///
    /// Returns:
    ///     - u16, the raw word data
    pub fn fetch_word(&self, word_index: u16, pointer: OOP) -> u16 {
        return self.heap_chunk_of_word(pointer, word_index);
    }

    /// Store a specific word of an object, without accounting for the header offset.
    ///
    /// Parameters:
    ///     - pointer: The pointer to the object
    ///     - word_index: The 0 indexed word that the value shall be stored at
    ///     - value: The value to be stored
    pub fn store_word(&mut self, word_index: u16, pointer: OOP, value: u16) {
        self.heap_chunk_of_word_put(pointer, word_index, value);
    }

    /// Fetch a specific byte of an object, without accounting for the header offset.
    ///
    /// Parameters:
    ///     - pointer: The pointer to the object
    ///     - byte_index: The 0 indexed byte that shall be fetched
    ///
    /// Returns:
    ///     - u16, the raw byte data
    pub fn fetch_byte(&self, byte_index: u16, pointer: OOP) -> u8 {
        return self.heap_chunk_of_byte(pointer, byte_index);
    }

    /// Store a specific byte of an object, without accounting for the header offset.
    ///
    /// Parameters:
    ///     - pointer: The pointer to the object
    ///     - byte_index: The 0 indexed byte that the value shall be stored at
    ///     - value: The value to be stored
    pub fn store_byte(&mut self, byte_index: u16, pointer: OOP, value: u8) {
        self.heap_chunk_of_byte_put(pointer, byte_index, value);
    }

    // ┌─────────────────────┐
    // │       LENGTH        │
    // └─────────────────────┘

    /// Fetch the length of an object in words.
    ///
    /// Parameters:
    ///     - pointer: The pointer to the object
    ///
    /// Returns:
    ///     - u16, the length of the object, in words, excluding the header.
    ///       (The amount of fields of the object)
    pub fn fetch_word_length_of(&self, oop: OOP) -> u16 {
        return self.size_bits_of(oop) - HEADER_SIZE;
    }

    /// Fetch the length of an object in bytes.
    ///
    /// Parameters:
    ///     - pointer: The pointer to the object
    ///
    /// Returns:
    ///     - u16, the length of the object, in bytes, excluding the header
    ///       and accounting for the odd bit.
    pub fn fetch_byte_length_of(&self, oop: OOP) -> u16 {
        if self.odd_bit_of(oop) {
            return (self.fetch_word_length_of(oop) * 2) - 1;
        } else {
            return self.fetch_word_length_of(oop) * 2;
        }
    }

    // ┌─────────────────────┐
    // │        CLASS        │
    // └─────────────────────┘

    /// Fetch the class information of an object.
    ///
    /// Parameters:
    ///     - pointer: The pointer to the object
    ///
    /// Returns:
    ///     - CLASS_SMALL_INTEGER_POINTER: pointer to the SmallInteger class if object is an
    ///     integer
    ///     - value of the class pointer field of the object otherwise.
    pub fn fetch_class_of(&self, pointer: OOP) -> u16 {
        if pointer.is_integer_object() {
            return CLASS_SMALL_INTEGER_POINTER;
        } else {
            return self.class_bits_of(pointer);
        }
    }

    /// The count of an object determines if it will be deallocated or not
    /// As long as one other object references it, it will not get deleted.
    /// If an object reaches more than 128 references it will be seen as
    /// 'saturated' and will never get deallocated.

    /// Increases the reference count of an OOP.
    ///
    /// If the object is an integer object, the count will not be increased.
    ///
    /// Parameters:
    ///     - oop: The pointer to the object
    pub fn increase_references_to(&mut self, oop: OOP) {
        if oop.is_integer_object() {
            return;
        }
        let cnt = self.count_bits_of(oop);
        if cnt < 128 {
            self.count_bits_of_put(oop, cnt + 1);
        }
    }

    /// Decreases the reference count of an OOP.
    ///
    /// If the object is an integer object, the count will not be decreased.
    ///
    /// If the count of an object reaches 0, it will get deallocated. When this happens
    /// all objects referenced by this object will get their count decremented, possibly
    /// cascading deletions.
    ///
    /// If the count of an object reaches 128, it is regarded as saturated and
    /// will not get the count decreased.
    ///
    /// Parameters:
    ///     - oop: The pointer to the object
    pub fn decrease_references_to(&mut self, oop: OOP) {
        if oop.is_integer_object() {
            return;
        }
        let mut cnt = self.count_bits_of(oop);
        if cnt >= 128 {
            // object is saturated -> permanent -> will never get deallocated
            return;
        }
        if cnt == 0 {
            // shouldn't really happen but necessary guard clause
            // if an object has a count of 0 it would already be deallocated
            return;
        }
        cnt = cnt - 1;
        if cnt == 0 {
            // the object will get deallocated
            let size = self.size_bits_of(oop);
            let last_pointer = if self.pointer_bit_of(oop) {
                size // all fields are pointers
            } else {
                HEADER_SIZE // only the class field can be a pointer
            };

            // decrement the reference counts of all referenced objects
            for i in 1..last_pointer {
                let field_oop = OOP::from_raw(self.heap_chunk_of_word(oop, i));
                self.decrease_references_to(field_oop);
            }

            self.deallocate(oop);
        } else {
            self.count_bits_of_put(oop, cnt);
        }
    }

    fn allocate_with_retries(&mut self, size: u16) -> Result<u16, ObjectMemoryError> {
        let mut result = self.attempt_to_allocate_chunk_in_current_segment(size);

        if result.is_err() {
            self.compact_current_segment();
            result = self.attempt_to_allocate_chunk_in_current_segment(size);
        }

        if result.is_err() {
            let original_segment = self.current_segment;
            for i in 0..HEAP_SIZE {
                if i == original_segment as usize {
                    continue;
                }
                self.current_segment = i as u8;
                result = self.attempt_to_allocate_chunk_in_current_segment(size);
                if result.is_err() {
                    self.compact_current_segment();
                    result = self.attempt_to_allocate_chunk_in_current_segment(size);
                }
                if result.is_ok() {
                    break;
                }
            }
        }

        return result;
    }

    pub fn instantiate_class_with_pointers(
        &mut self,
        class: u16,
        length: u16,
    ) -> Result<OOP, ObjectMemoryError> {
        let size = HEADER_SIZE + length;
        let result = self.allocate_with_retries(size);

        let oop = self.obtain_pointer(size, result?)?;
        self.class_bits_of_put(oop, class);
        self.pointer_bit_of_put(oop, true);
        for i in 0..length {
            self.heap_chunk_of_word_put(oop, i, NIL_POINTER);
        }
        return Ok(oop);
    }

    pub fn instantiate_class_with_words(
        &mut self,
        class: u16,
        length: u16,
    ) -> Result<OOP, ObjectMemoryError> {
        let size = HEADER_SIZE + length;
        let result = self.allocate_with_retries(size);

        let oop = self.obtain_pointer(size, result?)?;
        self.class_bits_of_put(oop, class);

        for i in 0..length {
            self.heap_chunk_of_word_put(oop, i, 0x0000);
        }
        return Ok(oop);
    }

    pub fn instantiate_class_with_bytes(
        &mut self,
        class: u16,
        length: u16,
    ) -> Result<OOP, ObjectMemoryError> {
        let size = HEADER_SIZE + (length + 1) / 2;
        let result = self.allocate_with_retries(size);

        let oop = self.obtain_pointer(size, result?)?;
        self.class_bits_of_put(oop, class);
        self.odd_bit_of_put(oop, length % 2 == 1);

        for i in 0..length {
            self.heap_chunk_of_byte_put(oop, i, 0x00);
        }
        return Ok(oop);
    }
}

#[cfg(test)]
mod api_accessor_tests {
    use crate::globalconstants::{BIG_SIZE, FIRST_FREE_CHUNK_LIST, NON_POINTER};

    use super::*;

    fn dummy_memory() -> ObjectMemory {
        let mut mem = ObjectMemory {
            heap: Heap::new(),
            object_table: vec![0u16; 64],
            free_pointer_list: NON_POINTER,
            current_segment: 0,
        };

        // initialize free chunk list heads
        for seg in 0..3 {
            for i in 0..=BIG_SIZE as usize {
                mem.heap[seg][(FIRST_FREE_CHUNK_LIST as usize) + i] = NON_POINTER;
            }
        }

        // OOP 0: segment 0, location 10, count=5
        mem.object_table[0] = 0x0500;
        mem.object_table[1] = 0x000A;

        // Heap data starting at 10
        mem.heap[0][10] = 0x0006; //size 6 words
        mem.heap[0][11] = 0x0020; // class pointer
        mem.heap[0][12] = 0xAAAA; // field 1
        mem.heap[0][13] = 0xBBBB; // field 2
        mem.heap[0][14] = 0xCCCC; // ...
        mem.heap[0][15] = 0xDDDD; // field {size} -2 (6-2=4)

        // OOP 2: segment 2, location 16, odd amount of bytes, count=5
        mem.object_table[2] = 0x0582;
        mem.object_table[3] = 0x0010;

        mem.heap[2][16] = 0x0004;
        mem.heap[2][17] = 0x0034;
        mem.heap[2][18] = 0xC0FE;
        mem.heap[2][19] = 0xBA00;

        return mem;
    }

    #[test]
    fn fetch_pointer_returns_correct_value() {
        let mem = dummy_memory();
        assert_eq!(mem.fetch_pointer(2, OOP::from_raw(0)), 0xCCCC);
        assert_eq!(mem.fetch_pointer(0, OOP::from_raw(2)), 0xC0FE);
    }

    #[test]
    fn store_pointer_stores_correct_value() {
        let mut mem = dummy_memory();
        // use SmallIntegers (odd values) to avoid ref counting OT lookups
        mem.heap[0][12] = 0x0003; // old value: SmallInteger
        mem.heap[2][18] = 0x0005; // old value: SmallInteger
        mem.store_pointer(0, OOP::from_raw(0), 0x0007); // new: SmallInteger
        mem.store_pointer(0, OOP::from_raw(2), 0x0009); // new: SmallInteger
        assert_eq!(mem.heap[0][12], 0x0007);
        assert_eq!(mem.heap[2][18], 0x0009);
    }

    #[test]
    fn fetch_word_returns_correct_value() {
        let mem = dummy_memory();
        assert_eq!(mem.fetch_word(1, OOP::from_raw(0)), 0x0020);
        assert_eq!(mem.fetch_word(3, OOP::from_raw(2)), 0xBA00);
    }

    #[test]
    fn store_word_stores_correct_value() {
        let mut mem = dummy_memory();
        mem.store_word(0, OOP::from_raw(0), 0xFFFF);
        mem.store_word(1, OOP::from_raw(2), 0xBEEF);
        assert_eq!(mem.heap[0][10], 0xFFFF);
        assert_eq!(mem.heap[2][17], 0xBEEF);
    }

    #[test]
    fn fetch_byte_returns_correct_value() {
        let mem = dummy_memory();
        assert_eq!(mem.fetch_byte(3, OOP::from_raw(0)), 0x20);
        assert_eq!(mem.fetch_byte(6, OOP::from_raw(2)), 0xBA);
    }

    #[test]
    fn store_byte_stores_correct_value() {
        let mut mem = dummy_memory();
        mem.store_byte(3, OOP::from_raw(0), 0x30);
        mem.store_byte(6, OOP::from_raw(2), 0xAB);
        assert_eq!(mem.heap[0][11], 0x0030);
        assert_eq!(mem.heap[2][19], 0xAB00);
    }

    #[test]
    fn fetch_word_length_of_returns_correct_value() {
        let mem = dummy_memory();
        assert_eq!(mem.fetch_word_length_of(OOP::from_raw(0)), 4);
        assert_eq!(mem.fetch_word_length_of(OOP::from_raw(2)), 2);
    }

    #[test]
    fn fetch_byte_length_of_returns_correct_value() {
        let mem = dummy_memory();
        assert_eq!(mem.fetch_byte_length_of(OOP::from_raw(0)), 8);
        assert_eq!(mem.fetch_byte_length_of(OOP::from_raw(2)), 3);
    }

    #[test]
    fn fetch_class_of_returns_correct_value() {
        let mem = dummy_memory();
        assert_eq!(mem.fetch_class_of(OOP::from_raw(0)), 0x0020);
        assert_eq!(
            mem.fetch_class_of(OOP::from_raw(1)),
            CLASS_SMALL_INTEGER_POINTER
        );
    }
}

#[cfg(test)]
mod ref_counting_tests {
    use crate::globalconstants::NON_POINTER;

    use super::*;

    fn dummy_memory() -> ObjectMemory {
        let mut mem = ObjectMemory {
            heap: Heap::new(),
            object_table: vec![0u16; 64],
            free_pointer_list: NON_POINTER,
            current_segment: 0,
        };

        // OOP 0: segment 0, location 22, count=5, pointer bit=1
        // A pointer object with 2 fields pointing to OOP 4 and OOP 6
        mem.object_table[0] = 0x0540; // count=5, P=1
        mem.object_table[1] = 0x0016; // location=22
        mem.heap[0][22] = 0x0004; // size 4 (header + 2 fields)
        mem.heap[0][23] = 0x0020; // class pointer
        mem.heap[0][24] = 0x0004; // field 0 -> OOP 4
        mem.heap[0][25] = 0x0006; // field 1 -> OOP 6

        // OOP 2: segment 0, location 30, count=3, pointer bit=0 (non-pointer object)
        mem.object_table[2] = 0x0300; // count=3, P=0
        mem.object_table[3] = 0x001E; // location=30
        mem.heap[0][30] = 0x0004; // size 4
        mem.heap[0][31] = 0x0020; // class pointer
        mem.heap[0][32] = 0x1234; // raw data, not a pointer
        mem.heap[0][33] = 0x5678; // raw data

        // OOP 4: segment 0, location 38, count=2
        mem.object_table[4] = 0x0200; // count=2
        mem.object_table[5] = 0x0026; // location=38
        mem.heap[0][38] = 0x0003; // size 3
        mem.heap[0][39] = 0x0020; // class
        mem.heap[0][40] = 0x0000; // one field

        // OOP 6: segment 0, location 46, count=1
        mem.object_table[6] = 0x0100; // count=1
        mem.object_table[7] = 0x002E; // location=46
        mem.heap[0][46] = 0x0002; // size 2 (header only, no fields)
        mem.heap[0][47] = 0x0020; // class

        // initialize free chunk list heads
        for i in 0..=BIG_SIZE as usize {
            mem.heap[0][(FIRST_FREE_CHUNK_LIST as usize) + i] = NON_POINTER;
        }

        return mem;
    }

    // ┌──────────────────────────────────────┐
    // │   increase_references_to tests       │
    // └──────────────────────────────────────┘

    #[test]
    fn increase_references_increments_count() {
        let mut mem = dummy_memory();
        mem.increase_references_to(OOP::from_raw(4));
        assert_eq!(mem.count_bits_of(OOP::from_raw(4)), 3);
    }

    #[test]
    fn increase_references_ignores_integers() {
        let mut mem = dummy_memory();
        // SmallInteger — should do nothing, no panic
        mem.increase_references_to(OOP::from_raw(3));
    }

    #[test]
    fn increase_references_saturates_at_128() {
        let mut mem = dummy_memory();
        mem.count_bits_of_put(OOP::from_raw(4), 128);
        mem.increase_references_to(OOP::from_raw(4));
        assert_eq!(mem.count_bits_of(OOP::from_raw(4)), 128);
    }

    #[test]
    fn increase_references_just_below_saturation() {
        let mut mem = dummy_memory();
        mem.count_bits_of_put(OOP::from_raw(4), 127);
        mem.increase_references_to(OOP::from_raw(4));
        assert_eq!(mem.count_bits_of(OOP::from_raw(4)), 128);
    }

    // ┌──────────────────────────────────────┐
    // │   decrease_references_to tests       │
    // └──────────────────────────────────────┘

    #[test]
    fn decrease_references_decrements_count() {
        let mut mem = dummy_memory();
        mem.decrease_references_to(OOP::from_raw(0));
        assert_eq!(mem.count_bits_of(OOP::from_raw(0)), 4);
    }

    #[test]
    fn decrease_references_ignores_integers() {
        let mut mem = dummy_memory();
        // SmallInteger — should do nothing, no panic
        mem.decrease_references_to(OOP::from_raw(3));
    }

    #[test]
    fn decrease_references_ignores_saturated() {
        let mut mem = dummy_memory();
        mem.count_bits_of_put(OOP::from_raw(4), 128);
        mem.decrease_references_to(OOP::from_raw(4));
        assert_eq!(mem.count_bits_of(OOP::from_raw(4)), 128);
    }

    #[test]
    fn decrease_references_deallocates_at_zero() {
        let mut mem = dummy_memory();
        // OOP 6 has count=1, decrementing should deallocate it
        mem.decrease_references_to(OOP::from_raw(6));
        assert!(mem.free_bit_of(OOP::from_raw(6)));
        assert_eq!(mem.free_pointer_list, 6);
    }

    #[test]
    fn decrease_references_cascades_on_pointer_object() {
        let mut mem = dummy_memory();
        // OOP 0 has count=5, P=1, fields pointing to OOP 4 (count=2) and OOP 6 (count=1)
        // Set OOP 0 count to 1 so it gets deallocated
        mem.count_bits_of_put(OOP::from_raw(0), 1);
        mem.decrease_references_to(OOP::from_raw(0));
        // OOP 0 should be deallocated
        assert!(mem.free_bit_of(OOP::from_raw(0)));
        // OOP 4 count should have gone from 2 to 1
        assert_eq!(mem.count_bits_of(OOP::from_raw(4)), 1);
        // OOP 6 had count=1, should also be deallocated (cascading)
        assert!(mem.free_bit_of(OOP::from_raw(6)));
    }

    #[test]
    fn decrease_references_no_cascade_on_nonpointer_object() {
        let mut mem = dummy_memory();
        // OOP 2 has count=3, P=0 (non-pointer), raw data 0x1234 and 0x5678
        // Set count to 1 so it gets deallocated
        mem.count_bits_of_put(OOP::from_raw(2), 1);
        mem.decrease_references_to(OOP::from_raw(2));
        // OOP 2 should be deallocated
        assert!(mem.free_bit_of(OOP::from_raw(2)));
        // The class (0x0020) gets decrease_references_to called on it
        // but 0x0020 is OOP 32 which doesn't exist in our test setup
        // — this tests that raw data fields are NOT treated as pointers
    }

    // ┌──────────────────────────────────────┐
    // │   store_pointer ref counting tests   │
    // └──────────────────────────────────────┘

    #[test]
    fn store_pointer_increments_new_decrements_old() {
        let mut mem = dummy_memory();
        // OOP 0 field 0 currently points to OOP 4 (count=2)
        // Store OOP 6 (count=1) into field 0
        mem.store_pointer(0, OOP::from_raw(0), 0x0006);
        // OOP 6 count should go from 1 to 2 (increased)
        assert_eq!(mem.count_bits_of(OOP::from_raw(6)), 2);
        // OOP 4 count should go from 2 to 1 (decreased)
        assert_eq!(mem.count_bits_of(OOP::from_raw(4)), 1);
        // field should now hold OOP 6
        assert_eq!(mem.fetch_pointer(0, OOP::from_raw(0)), 0x0006);
    }

    #[test]
    fn store_pointer_same_value_no_change() {
        let mut mem = dummy_memory();
        // Store same OOP 4 back — count should stay the same
        // increase then decrease = net zero
        let old_count = mem.count_bits_of(OOP::from_raw(4));
        mem.store_pointer(0, OOP::from_raw(0), 0x0004);
        assert_eq!(mem.count_bits_of(OOP::from_raw(4)), old_count);
    }
}

// ====================================
//  Allocator Functionality
// ====================================

impl ObjectMemory {
    fn attempt_to_allocate_chunk_in_current_segment(
        &mut self,
        size: u16,
    ) -> Result<u16, ObjectMemoryError> {
        let seg = self.current_segment as usize;

        // Attempt to allocate in the 'fitting' list first.
        if size < BIG_SIZE {
            // Try the 'fitting' list
            if let Ok(location) = self.remove_from_free_chunk_list(self.current_segment, size) {
                return Ok(location);
            }
            // Fall through if allocation was not possible
        }

        // Allocate a chunk in the big list
        let big_list_index = (FIRST_FREE_CHUNK_LIST + BIG_SIZE) as usize;
        // Start at the 'top' of the list, if empty current == NON_POINTER
        let mut current = self.heap[seg][big_list_index];
        // Since we start at the 'top' of the list, the previous entry is None
        let mut prev_location: Option<u16> = None;

        while current != NON_POINTER {
            let chunk_size = self.heap[seg][current as usize];
            if chunk_size >= size {
                // we found a match, unlink it from the list
                let next = self.heap[seg][(current + 1) as usize];
                match prev_location {
                    // If we were at the top, set the 'top' of the list, to the next entry
                    // -> removing the top, so this item in the process
                    None => self.heap[seg][big_list_index] = next,
                    // If we're in the middle of the list, set the next item of the previous item
                    // to this item's next item, removing this item from the chain
                    Some(prev) => self.heap[seg][(prev + 1) as usize] = next,
                }

                // split the remainder size if it makes sense (compact if remainder > HEADER_SIZE)
                let remainder = chunk_size - size;
                if remainder >= HEADER_SIZE {
                    // the remainder starts after our chunk ends, so after size words
                    let remainder_location = current + size;
                    // free the remainder again with the usual logic
                    self.to_free_chunk_list_add(
                        self.current_segment,
                        remainder,
                        remainder_location,
                    );
                }
                // we found a chunk, return it
                return Ok(current);
            }

            // set iteration variables and continue iterating
            prev_location = Some(current);
            current = self.heap[seg][(current + 1) as usize];
        }

        // we found no applicable chunk
        return Err(ObjectMemoryError::NoFreeEntries);
    }

    /// Fetch a free pointer and return a new 'fresh' object
    ///
    /// Parameters:
    ///     - size: the size of the new object (number of fields)
    ///     - location: the 'requested' location for the new object
    ///
    /// Returns:
    ///     - Pointer to the new object
    ///     - ObjectMemoryError if no free memory is available
    fn obtain_pointer(&mut self, size: u16, location: u16) -> Result<OOP, ObjectMemoryError> {
        let oop = self.remove_from_free_pointer_list()?;
        self.segment_bits_of_put(oop, self.current_segment);
        self.location_bits_of_put(oop, location);
        self.size_bits_of_put(oop, size);
        return Ok(oop);
    }

    /// 'Removes' the object from memory
    ///
    /// Parameters:
    ///     - oop: The pointer to the object that shall be erased
    fn deallocate(&mut self, oop: OOP) {
        let size = self.size_bits_of(oop);
        let loc = self.location_bits_of(oop);
        let seg = self.segment_bits_of(oop);
        self.to_free_pointer_list_add(oop);
        self.to_free_chunk_list_add(seg, size, loc);
    }

    /// Clears all free lists by setting the top to NON_POINTER
    fn abandon_free_chunks_in_segment(&mut self, segment: u8) {
        for i in 0..=BIG_SIZE {
            self.heap[segment as usize][(FIRST_FREE_CHUNK_LIST + i) as usize] = NON_POINTER
        }
    }

    fn compact_current_segment(&mut self) {
        // 'forget' all free chunks since we'll realign anyways
        self.abandon_free_chunks_in_segment(self.current_segment);

        // temporary structure to keep track of things
        struct Entry {
            oop: OOP,
            location: u16,
            size: u16,
        };

        let mut entries: Vec<Entry> = Vec::new();

        // loop through the object table, take a look at each OOP (every second word)
        for i in (0..self.object_table.len()).step_by(2) {
            let oop = OOP::from_raw(i as u16);

            // If it is not free (so an object) and part of the segment we're currently compacting
            // save it to the list
            if !self.free_bit_of(oop) && self.segment_bits_of(oop) == self.current_segment {
                entries.push(Entry {
                    oop: oop,
                    location: self.location_bits_of(oop),
                    size: self.size_bits_of(oop),
                });
            }
        }

        // sort the list in ascending order of the location
        entries.sort_by_key(|e| e.location);

        // slide the elements to the front to free up space at the back
        let mut low_mark = LAST_BIG_CHUNK_LIST + 1;
        for entry in &entries {
            if entry.location > low_mark {
                for i in 0..entry.size {
                    self.heap[self.current_segment as usize][(low_mark + i) as usize] =
                        self.heap[self.current_segment as usize][(entry.location + i) as usize]
                }
                self.location_bits_of_put(entry.oop, low_mark);
            }
            if entry.location == low_mark {
                // do nothing, entry is already in place
            }
            if entry.location < low_mark {
                // we fucked up somehow
                panic!("Compacting failed, sorted entries were not sorted");
            }
            low_mark += entry.size;
        }

        if let Some(last) = entries.last() {
            let old_end = last.location + last.size;
            let remaining = old_end - low_mark;
            if remaining > 0 {
                self.to_free_chunk_list_add(self.current_segment, remaining, low_mark);
            }
        }
    }
}

#[cfg(test)]
mod allocator_tests {
    use crate::globalconstants::{BIG_SIZE, FIRST_FREE_CHUNK_LIST, HEADER_SIZE, NON_POINTER};

    use super::*;

    fn dummy_memory() -> ObjectMemory {
        let mut mem = ObjectMemory {
            heap: Heap::new(),
            object_table: vec![0u16; 64],
            free_pointer_list: NON_POINTER,
            current_segment: 0,
        };
        // mark all OT entries as free so compaction doesn't pick up garbage
        for i in (0..64).step_by(2) {
            mem.free_bit_of_put(OOP::from_raw(i as u16), true);
        }
        // initialize free chunk list heads to NON_POINTER (empty)
        for i in 0..=(BIG_SIZE as usize) {
            mem.heap[0][(FIRST_FREE_CHUNK_LIST as usize) + i] = NON_POINTER;
        }
        return mem;
    }

    // ┌──────────────────────────────────────────────────────┐
    // │  attempt_to_allocate_chunk_in_current_segment tests  │
    // └──────────────────────────────────────────────────────┘

    #[test]
    fn allocate_chunk_exact_fit() {
        let mut memory = dummy_memory();
        // add a free chunk of size 4 at location 30
        memory.to_free_chunk_list_add(0, 4, 30);
        let result = memory.attempt_to_allocate_chunk_in_current_segment(4);
        assert_eq!(result, Ok(30));
    }

    #[test]
    fn allocate_chunk_from_big_list() {
        let mut memory = dummy_memory();
        // add a free chunk of size 25 at location 40
        memory.to_free_chunk_list_add(0, 25, 40);
        let result = memory.attempt_to_allocate_chunk_in_current_segment(25);
        assert_eq!(result, Ok(40));
    }

    #[test]
    fn allocate_chunk_splits_big_chunk() {
        let mut memory = dummy_memory();
        // add a free chunk of size 30 at location 30 (goes to big list)
        memory.to_free_chunk_list_add(0, 30, 30);
        // request size 4 — should split, returning the lower part
        let result = memory.attempt_to_allocate_chunk_in_current_segment(4);
        assert_eq!(result, Ok(30));
        // remainder (size 26) should be on the big free list
        assert_eq!(
            memory.heap[0][(FIRST_FREE_CHUNK_LIST + BIG_SIZE) as usize],
            34
        );
        assert_eq!(memory.heap[0][34], 26); // remainder size
    }

    #[test]
    fn allocate_chunk_no_split_when_remainder_too_small() {
        let mut memory = dummy_memory();
        // add a free chunk of size 5 at location 30
        memory.to_free_chunk_list_add(0, 5, 30);
        // request size 4 — remainder would be 1, less than HEADER_SIZE, so no split
        let result = memory.attempt_to_allocate_chunk_in_current_segment(5);
        assert_eq!(result, Ok(30));
    }

    #[test]
    fn allocate_chunk_falls_through_to_big_list() {
        let mut memory = dummy_memory();
        // no exact-fit list for size 4, but a big chunk exists
        memory.to_free_chunk_list_add(0, 22, 50);
        let result = memory.attempt_to_allocate_chunk_in_current_segment(4);
        assert_eq!(result, Ok(50));
        // remainder (size 18) should be on the free list
        assert_eq!(memory.heap[0][(FIRST_FREE_CHUNK_LIST + 18) as usize], 54);
    }

    #[test]
    fn allocate_chunk_returns_error_when_empty() {
        let mut memory = dummy_memory();
        let result = memory.attempt_to_allocate_chunk_in_current_segment(4);
        assert!(result.is_err());
    }

    // ┌──────────────────────────────┐
    // │     obtain_pointer tests     │
    // └──────────────────────────────┘

    #[test]
    fn obtain_pointer_sets_up_ot_entry() {
        let mut memory = dummy_memory();
        // add a free OT entry
        memory.to_free_pointer_list_add(OOP::from_raw(4));
        let result = memory.obtain_pointer(6, 30);
        assert!(result.is_ok());
        let oop = result.unwrap();
        assert_eq!(oop, OOP::from_raw(4));
        assert_eq!(memory.size_bits_of(oop), 6);
        assert_eq!(memory.segment_bits_of(oop), 0); // current_segment
        assert_eq!(memory.location_bits_of(oop), 30);
        assert!(!memory.free_bit_of(oop));
    }

    #[test]
    fn obtain_pointer_returns_error_when_no_free_entries() {
        let mut memory = dummy_memory();
        let result = memory.obtain_pointer(6, 30);
        assert!(result.is_err());
    }

    // ┌──────────────────────────────┐
    // │      deallocate tests        │
    // └──────────────────────────────┘

    #[test]
    fn deallocate_frees_ot_entry_and_heap_chunk() {
        let mut memory = dummy_memory();
        // set up a live object: OOP 4, segment 0, location 30, size 6
        memory.object_table[4] = 0x0000; // count=0, segment=0
        memory.object_table[5] = 30; // location=30
        memory.heap[0][30] = 6; // size
        memory.heap[0][31] = 0x0020; // class

        memory.deallocate(OOP::from_raw(4));

        // OT entry should be on free pointer list
        assert!(memory.free_bit_of(OOP::from_raw(4)));
        assert_eq!(memory.free_pointer_list, 4);
        // heap chunk should be on free chunk list for size 6
        assert_eq!(memory.heap[0][(FIRST_FREE_CHUNK_LIST + 6) as usize], 30);
    }

    #[test]
    fn deallocate_then_reallocate() {
        let mut memory = dummy_memory();
        // set up a live object: OOP 4, segment 0, location 30, size 6
        memory.object_table[4] = 0x0000;
        memory.object_table[5] = 30;
        memory.heap[0][30] = 6;
        memory.heap[0][31] = 0x0020;

        memory.deallocate(OOP::from_raw(4));

        // should be able to get the OT entry back
        let oop = memory.remove_from_free_pointer_list();
        assert_eq!(oop, Ok(OOP::from_raw(4)));
        // should be able to get the chunk back
        let chunk = memory.remove_from_free_chunk_list(0, 6);
        assert_eq!(chunk, Ok(30));
    }

    // ┌──────────────────────────────────────────────────┐
    // │     abandon_free_chunks_in_segment tests         │
    // └──────────────────────────────────────────────────┘

    #[test]
    fn abandon_clears_all_free_chunk_lists() {
        let mut memory = dummy_memory();
        // add chunks to several lists
        memory.to_free_chunk_list_add(0, 4, 100);
        memory.to_free_chunk_list_add(0, 10, 200);
        memory.to_free_chunk_list_add(0, 25, 300); // big list

        memory.abandon_free_chunks_in_segment(0);

        // all list heads should be NON_POINTER
        for i in 0..=BIG_SIZE as usize {
            assert_eq!(
                memory.heap[0][(FIRST_FREE_CHUNK_LIST as usize) + i],
                NON_POINTER,
                "free list head at index {} was not cleared",
                i
            );
        }
    }

    #[test]
    fn abandon_only_affects_target_segment() {
        let mut memory = dummy_memory();
        // initialize segment 1 free list heads
        for i in 0..=(BIG_SIZE as usize) {
            memory.heap[1][(FIRST_FREE_CHUNK_LIST as usize) + i] = NON_POINTER;
        }
        // add chunks to both segments
        memory.to_free_chunk_list_add(0, 4, 100);
        memory.to_free_chunk_list_add(1, 4, 100);

        memory.abandon_free_chunks_in_segment(0);

        // segment 0 should be cleared
        assert_eq!(
            memory.heap[0][(FIRST_FREE_CHUNK_LIST + 4) as usize],
            NON_POINTER
        );
        // segment 1 should still have its chunk
        assert_eq!(memory.heap[1][(FIRST_FREE_CHUNK_LIST + 4) as usize], 100);
    }

    // ┌──────────────────────────────────────────────���───┐
    // │        compact_current_segment tests             │
    // └──────────────────────────────────────────────────┘

    #[test]
    fn compact_slides_objects_down() {
        let mut memory = dummy_memory();
        // Object A: OOP 0, location 100, size 4
        memory.object_table[0] = 0x0100; // count=1
        memory.object_table[1] = 100;
        memory.heap[0][100] = 4; // size
        memory.heap[0][101] = 0x0020; // class
        memory.heap[0][102] = 0xAAAA;
        memory.heap[0][103] = 0xBBBB;

        // Object B: OOP 2, location 200, size 3
        memory.object_table[2] = 0x0100; // count=1
        memory.object_table[3] = 200;
        memory.heap[0][200] = 3;
        memory.heap[0][201] = 0x0020;
        memory.heap[0][202] = 0xCCCC;

        memory.compact_current_segment();

        // Both objects should be slid to the front (after free list heads)
        let base = (LAST_BIG_CHUNK_LIST + 1) as usize;

        // Object A should be at base
        assert_eq!(memory.location_bits_of(OOP::from_raw(0)), base as u16);
        assert_eq!(memory.heap[0][base], 4);
        assert_eq!(memory.heap[0][base + 1], 0x0020);
        assert_eq!(memory.heap[0][base + 2], 0xAAAA);
        assert_eq!(memory.heap[0][base + 3], 0xBBBB);

        // Object B should follow directly after A
        let b_base = base + 4;
        assert_eq!(memory.location_bits_of(OOP::from_raw(2)), b_base as u16);
        assert_eq!(memory.heap[0][b_base], 3);
        assert_eq!(memory.heap[0][b_base + 1], 0x0020);
        assert_eq!(memory.heap[0][b_base + 2], 0xCCCC);
    }

    #[test]
    fn compact_already_packed_is_noop() {
        let mut memory = dummy_memory();
        let base = (LAST_BIG_CHUNK_LIST + 1) as u16;

        // Object already at the lowest possible location
        memory.object_table[0] = 0x0100; // count=1
        memory.object_table[1] = base;
        memory.heap[0][base as usize] = 3;
        memory.heap[0][(base + 1) as usize] = 0x0020;
        memory.heap[0][(base + 2) as usize] = 0x1234;

        memory.compact_current_segment();

        // Should not have moved
        assert_eq!(memory.location_bits_of(OOP::from_raw(0)), base);
        assert_eq!(memory.heap[0][(base + 2) as usize], 0x1234);
    }

    #[test]
    fn compact_reclaims_free_space() {
        let mut memory = dummy_memory();
        // Object: OOP 0, location 200, size 4
        memory.object_table[0] = 0x0100;
        memory.object_table[1] = 200;
        memory.heap[0][200] = 4;
        memory.heap[0][201] = 0x0020;
        memory.heap[0][202] = 0xAAAA;
        memory.heap[0][203] = 0xBBBB;

        memory.compact_current_segment();

        let base = (LAST_BIG_CHUNK_LIST + 1) as u16;
        let free_start = base + 4;
        // The gap between original end (204) and new end should be a free chunk
        let free_size = 204 - free_start;
        // free chunk should be on the appropriate list
        if free_size >= BIG_SIZE {
            assert_eq!(
                memory.heap[0][(FIRST_FREE_CHUNK_LIST + BIG_SIZE) as usize],
                free_start
            );
        } else {
            assert_eq!(
                memory.heap[0][(FIRST_FREE_CHUNK_LIST + free_size) as usize],
                free_start
            );
        }
    }

    #[test]
    fn compact_skips_free_entries() {
        let mut memory = dummy_memory();
        // OOP 0: live object at location 100
        memory.object_table[0] = 0x0100; // count=1
        memory.object_table[1] = 100;
        memory.heap[0][100] = 3;
        memory.heap[0][101] = 0x0020;
        memory.heap[0][102] = 0xAAAA;

        // OOP 2: free entry (should be ignored)
        memory.free_bit_of_put(OOP::from_raw(2), true);

        // OOP 4: live object at location 200
        memory.object_table[4] = 0x0100; // count=1
        memory.object_table[5] = 200;
        memory.heap[0][200] = 3;
        memory.heap[0][201] = 0x0020;
        memory.heap[0][202] = 0xBBBB;

        memory.compact_current_segment();

        let base = (LAST_BIG_CHUNK_LIST + 1) as usize;
        // OOP 0 at base
        assert_eq!(memory.location_bits_of(OOP::from_raw(0)), base as u16);
        // OOP 4 right after
        assert_eq!(memory.location_bits_of(OOP::from_raw(4)), (base + 3) as u16);
        // OOP 2 still free
        assert!(memory.free_bit_of(OOP::from_raw(2)));
    }

    #[test]
    fn compact_skips_other_segments() {
        let mut memory = dummy_memory();
        memory.current_segment = 0;

        // OOP 0: segment 0, location 100
        memory.object_table[0] = 0x0100; // count=1, segment=0
        memory.object_table[1] = 100;
        memory.heap[0][100] = 3;
        memory.heap[0][101] = 0x0020;
        memory.heap[0][102] = 0xAAAA;

        // OOP 2: segment 1, location 100
        memory.object_table[2] = 0x0101; // count=1, segment=1
        memory.object_table[3] = 100;
        memory.heap[1][100] = 3;
        memory.heap[1][101] = 0x0020;
        memory.heap[1][102] = 0xBBBB;

        memory.compact_current_segment();

        // OOP 0 should have moved (segment 0 was compacted)
        let base = (LAST_BIG_CHUNK_LIST + 1) as u16;
        assert_eq!(memory.location_bits_of(OOP::from_raw(0)), base);
        // OOP 2 should be untouched (segment 1)
        assert_eq!(memory.location_bits_of(OOP::from_raw(2)), 100);
    }
}

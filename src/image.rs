use std::{
    fs::write,
    path::{Path, PathBuf},
};

use crate::{
    errors::ImageLoadError,
    globalconstants::{
        CLASS_ARRAY_POINTER, CLASS_FLOAT_POINTER, CLASS_SMALL_INTEGER_POINTER,
        CLASS_STRING_POINTER, FALSE_POINTER, HEAP_SEGMENT_SIZE, IMAGE_HEADER_BYTES,
        IMAGE_PAGE_SIZE_BYTES, NIL_POINTER, SCHEDULER_ASSOCIATION_POINTER, TRUE_POINTER,
    },
    objectmemory::ObjectMemory,
    oop::OOP,
};

#[derive(Debug)]
pub struct Image {
    path: PathBuf,
    data: Vec<u8>,
    header: ImageHeader,
    big_endian: bool,
}

#[derive(Debug, Clone)]
pub struct ImageHeader {
    pub last_segment: u16,
    pub limit_in_last_page: u16,
    pub object_table_length: u32,
    pub image_type: u16,
}

impl Image {
    /// Reads an .im image file from disk and parses its 512-byte header.
    ///
    /// Endianness is detected by interpreting the object table length field
    /// as big-endian: since a valid OT length fits in u16, a BE u32 that
    /// exceeds 0xFFFF indicates LE encoding instead.
    ///
    /// # Parameters:
    ///     * path (PathBuf): The filesystem path to the .im file
    ///
    /// # Returns:
    ///     * Result<Image, ImageLoadError>, the loaded image with its decoded
    ///       header, or an error if the file is too short, the image type is
    ///       unsupported, or I/O failed
    pub fn load(path: PathBuf) -> Result<Image, ImageLoadError> {
        let data = std::fs::read(&path)?;

        if data.len() < IMAGE_HEADER_BYTES {
            return Err(ImageLoadError::InvalidHeader(
                "Image file should be at least contain the header (512 Bytes)".to_string(),
            ));
        }

        let mut object_table_length = u32::from_be_bytes([data[4], data[5], data[6], data[7]]); // word 2 and 3

        let big_endian = object_table_length <= 0xFFFF;

        let last_segment: u16;
        let limit_in_last_page: u16;
        let image_type: u16;
        let heap_length: u32;

        if big_endian {
            heap_length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            last_segment = (heap_length >> 16) as u16; // word 0
            limit_in_last_page = (heap_length & 0xFFFF) as u16; // word 1
            image_type = u16::from_be_bytes([data[8], data[9]]); // word 4
        } else {
            heap_length = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            object_table_length = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            last_segment = (heap_length >> 16) as u16; // word 0
            limit_in_last_page = (heap_length & 0xFFFF) as u16; // word 1
            image_type = u16::from_le_bytes([data[8], data[9]]); // word 4
        }

        match image_type {
            0 => {}
            1 => {
                return Err(ImageLoadError::UnsupportedImageType(
                    "The Stretch/DV6 image format is not supported".into(),
                ));
            }
            other => {
                return Err(ImageLoadError::UnsupportedImageType(format!(
                    "unknown image type: {}",
                    other
                )));
            }
        }

        let header = ImageHeader {
            last_segment,
            limit_in_last_page,
            object_table_length,
            image_type,
        };

        let image = Image {
            path,
            data,
            header,
            big_endian,
        };

        Ok(image)
    }

    /// Parses the loaded image's heap and object table into an ObjectMemory.
    ///
    /// The heap follows the 512-byte header; the object table starts at the
    /// next 512-byte page boundary after the heap. Endianness follows the
    /// value detected at load time. Validates that the well-known OOPs
    /// (nil, true, false, scheduler association, primitive class pointers)
    /// are present and not marked free.
    ///
    /// The returned ObjectMemory is ready for read-only inspection. Before
    /// any allocation, call `ObjectMemory::rebuild_free_pointer_list`.
    ///
    /// # Parameters:
    ///     * image (&Image): The image whose bytes should be parsed
    ///
    /// # Returns:
    ///     * Result<ObjectMemory, ImageLoadError>, the parsed memory, or an
    ///       InvalidImage error if a well-known OOP is missing or marked free
    pub fn parse_into_memory(image: &Image) -> Result<ObjectMemory, ImageLoadError> {
        let mut heap_segments: Vec<Vec<u16>> = Vec::new();
        let mut heap_segment: Vec<u16> = Vec::with_capacity(HEAP_SEGMENT_SIZE);

        let heap_words = (image.header.last_segment as usize) * HEAP_SEGMENT_SIZE
            + image.header.limit_in_last_page as usize;
        let heap_end = IMAGE_HEADER_BYTES + heap_words * 2;

        for chunk in image.data[IMAGE_HEADER_BYTES..heap_end].chunks_exact(2) {
            let heap_word: u16;

            if image.big_endian {
                heap_word = u16::from_be_bytes([chunk[0], chunk[1]]);
            } else {
                heap_word = u16::from_le_bytes([chunk[0], chunk[1]]);
            }

            heap_segment.push(heap_word);
            if heap_segment.len() == HEAP_SEGMENT_SIZE {
                heap_segments.push(std::mem::take(&mut heap_segment));
            }
        }
        if !heap_segment.is_empty() {
            heap_segments.push(heap_segment);
        }

        // we now finished parsing the heap. Lets jump to the beginning of the new page
        let index = heap_end.div_ceil(IMAGE_PAGE_SIZE_BYTES) * IMAGE_PAGE_SIZE_BYTES;

        let ot_bytes = image.header.object_table_length as usize * 2;
        let ot_end = index + ot_bytes;

        let mut object_table: Vec<u16> =
            Vec::with_capacity(image.header.object_table_length as usize);

        for entry_bytes in image.data[index..ot_end].chunks_exact(2) {
            let word: u16;

            if image.big_endian {
                word = u16::from_be_bytes([entry_bytes[0], entry_bytes[1]])
            } else {
                word = u16::from_le_bytes([entry_bytes[0], entry_bytes[1]])
            }

            object_table.push(word);
        }

        let mem = ObjectMemory::from_image(heap_segments, object_table);

        for (oop_value, name) in [
            (NIL_POINTER, "nil"),
            (FALSE_POINTER, "false"),
            (TRUE_POINTER, "true"),
            (SCHEDULER_ASSOCIATION_POINTER, "SchedulerAssociation"),
            (CLASS_SMALL_INTEGER_POINTER, "SmallInteger class"),
            (CLASS_STRING_POINTER, "String class"),
            (CLASS_ARRAY_POINTER, "Array class"),
            (CLASS_FLOAT_POINTER, "Float class"),
        ] {
            if (oop_value as usize) >= mem.object_table_len()
                || mem.is_free_oop(OOP::from_raw(oop_value))
            {
                return Err(ImageLoadError::InvalidImage(format!(
                    "well-known OOP {} ({:#x}) is missing or marked free",
                    name, oop_value
                )));
            }
        }
        return Ok(mem);
    }

    /// Returns a reference to the decoded image header.
    ///
    /// # Returns:
    ///     * &ImageHeader, the four decoded header fields (heap dimensions,
    ///       OT length, image type)
    pub fn header(&self) -> &ImageHeader {
        &self.header
    }

    /// Returns whether the image is big-endian encoded.
    ///
    /// # Returns:
    ///     * bool, true if the image uses big-endian word ordering
    pub fn big_endian(&self) -> bool {
        self.big_endian
    }

    /// Returns the total size of the loaded image file in bytes.
    ///
    /// # Returns:
    ///     * usize, the number of bytes that were read from disk
    pub fn data_len(&self) -> usize {
        self.data.len()
    }

    /// Returns a reference to the raw bytes of the loaded image file.
    ///
    /// # Returns:
    ///     * &[u8], the unmodified bytes as read from disk
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Serialises an ObjectMemory back into the .im file byte format.
    ///
    /// Heap dimensions are taken from `mem`; image type and endianness are
    /// taken from this Image's original header. Layout mirrors the load
    /// side: 512-byte header, heap (zero-padded to the next page boundary),
    /// then object table. For an unmutated load → dump roundtrip the
    /// resulting bytes match the original file exactly.
    ///
    /// # Parameters:
    ///     * mem (&ObjectMemory): The object memory to serialise
    ///
    /// # Returns:
    ///     * Vec<u8>, the serialised image bytes
    pub fn dump(&self, mem: &ObjectMemory) -> Vec<u8> {
        // how many segments? (zero indexed)
        let last_seg = mem.last_segment();
        // how many words are actually in the last segment?
        let limit = mem.limit_in_last_page();
        // total words contained in the heap
        let heap_words = last_seg as usize * HEAP_SEGMENT_SIZE + limit as usize;
        // words in the object table
        let ot_words = mem.object_table_len();
        // where in the image does the heap end?
        let heap_end = IMAGE_HEADER_BYTES + heap_words as usize * 2;
        // Where does the object table start? This value holds the index of the last byte of the
        // 'last' page that the heap occupies
        let ot_offset = heap_end.div_ceil(IMAGE_PAGE_SIZE_BYTES) * IMAGE_PAGE_SIZE_BYTES;
        // total image size (calculated)
        let total = ot_offset + ot_words * 2;

        let mut out = Vec::with_capacity(total);

        // helper functions for insertion, here as closures
        fn push_u16(out: &mut Vec<u8>, v: u16, big_endian: bool) {
            if big_endian {
                out.extend_from_slice(&v.to_be_bytes())
            } else {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }

        fn push_u32(out: &mut Vec<u8>, v: u32, big_endian: bool) {
            if big_endian {
                out.extend_from_slice(&v.to_be_bytes())
            } else {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }

        // part one: The Header

        // heap_length is a 2 word value (32 bit) containing the last segment in the high word, and
        // the limit in the low word
        let heap_length = ((last_seg as u32) << 16) | (limit as u32);
        push_u32(&mut out, heap_length, self.big_endian);
        push_u32(&mut out, ot_words as u32, self.big_endian);
        push_u16(&mut out, self.header.image_type, self.big_endian);
        // pad with 0x00 to end of the first page
        out.resize(IMAGE_HEADER_BYTES, 0);

        // part two: The heap

        for seg in 0..=last_seg {
            let seg_len = if seg == last_seg {
                limit as usize
            } else {
                HEAP_SEGMENT_SIZE
            };
            for word in 0..seg_len {
                push_u16(
                    &mut out,
                    mem.heap_segments()[seg as usize][word],
                    self.big_endian,
                );
            }
        }

        // pad with 0x00 to the next page boundry
        out.resize(ot_offset, 0);

        // part three: The object table
        for &word in mem.object_table() {
            push_u16(&mut out, word, self.big_endian);
        }

        return out;
    }

    /// Dumps an ObjectMemory and writes the resulting bytes to disk.
    ///
    /// # Parameters:
    ///     * mem (&ObjectMemory): The object memory to serialise
    ///     * path (&Path): The filesystem destination
    ///
    /// # Returns:
    ///     * std::io::Result<()>, Ok if the write succeeded
    pub fn save(&self, mem: &ObjectMemory, path: &Path) -> std::io::Result<()> {
        write(path, self.dump(mem))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testBinaries")
            .join(name)
    }

    #[test]
    fn loads_valid_big_endian_image() {
        let img = Image::load(fixture("valid_be.im")).expect("load failed");
        assert!(img.big_endian);
        assert_eq!(img.header.last_segment, 1);
        assert_eq!(img.header.limit_in_last_page, 0xFF);
        assert_eq!(img.header.object_table_length, 0x1000);
        assert_eq!(img.header.image_type, 0);
    }

    #[test]
    fn loads_valid_little_endian_image() {
        let img = Image::load(fixture("valid_le.im")).expect("load failed");
        assert!(!img.big_endian);
        assert_eq!(img.header.last_segment, 1);
        assert_eq!(img.header.limit_in_last_page, 0xFF);
        assert_eq!(img.header.object_table_length, 0x1000);
        assert_eq!(img.header.image_type, 0);
    }

    #[test]
    fn rejects_too_short_file() {
        let result = Image::load(fixture("too_short.im"));
        assert!(matches!(result, Err(ImageLoadError::InvalidHeader(_))));
    }

    #[test]
    fn rejects_dv6_image_type() {
        let result = Image::load(fixture("type_dv6.im"));
        assert!(matches!(
            result,
            Err(ImageLoadError::UnsupportedImageType(_))
        ));
    }

    #[test]
    fn rejects_unknown_image_type() {
        let result = Image::load(fixture("type_unknown.im"));
        assert!(matches!(
            result,
            Err(ImageLoadError::UnsupportedImageType(_))
        ));
    }

    #[test]
    fn returns_io_error_for_missing_file() {
        let bogus = PathBuf::from("/nonexistent/path/no_such_file.im");
        let result = Image::load(bogus);
        assert!(matches!(result, Err(ImageLoadError::Io(_))));
    }

    #[test]
    fn parse_into_memory_loads_big_endian_image() {
        use crate::oop::OOP;
        let img = Image::load(fixture("parse_be.im")).expect("load failed");
        let memory = Image::parse_into_memory(&img).expect("parse failed");
        // OOP 0 points to segment 0, location 0; class word lives at heap[0][1] = 0x000C
        assert_eq!(memory.fetch_class_of(OOP::from_raw(0)), 0x000C);
    }

    #[test]
    fn parse_into_memory_loads_little_endian_image() {
        use crate::oop::OOP;
        let img = Image::load(fixture("parse_le.im")).expect("load failed");
        let memory = Image::parse_into_memory(&img).expect("parse failed");
        // Same logical content as the BE fixture
        assert_eq!(memory.fetch_class_of(OOP::from_raw(0)), 0x000C);
    }

    #[test]
    fn parse_into_memory_be_and_le_produce_equal_memory() {
        let img_be = Image::load(fixture("parse_be.im")).expect("load BE failed");
        let img_le = Image::load(fixture("parse_le.im")).expect("load LE failed");
        let mem_be = Image::parse_into_memory(&img_be).expect("parse BE failed");
        let mem_le = Image::parse_into_memory(&img_le).expect("parse LE failed");
        assert_eq!(mem_be, mem_le);
    }

    #[test]
    fn loads_xerox_v2_snapshot() {
        use crate::globalconstants::CLASS_METHOD_CONTEXT_POINTER;

        let img = Image::load(fixture("snapshot.im")).expect("load failed");
        let mem = Image::parse_into_memory(&img).expect("parse failed");

        // Walk the bootstrap chain documented in Bluebook ch.30:
        //   SchedulerAssociation -> ProcessorScheduler -> active Process -> suspended MethodContext
        // All three hops use field index 1 (value / activeProcess / suspendedContext).
        let scheduler = mem.fetch_pointer(1, OOP::from_raw(SCHEDULER_ASSOCIATION_POINTER));
        let active_process = mem.fetch_pointer(1, OOP::from_raw(scheduler));
        let initial_context = mem.fetch_pointer(1, OOP::from_raw(active_process));

        assert_eq!(
            mem.fetch_class_of(OOP::from_raw(initial_context)),
            CLASS_METHOD_CONTEXT_POINTER,
            "the active process's suspended context must be a MethodContext"
        );
    }
}

#[cfg(test)]
mod dump_tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testBinaries")
            .join(name)
    }

    fn assert_dump_roundtrip(fixture_name: &str) {
        let img = Image::load(fixture(fixture_name)).expect("load failed");
        let mem = Image::parse_into_memory(&img).expect("parse failed");
        let dumped = img.dump(&mem);
        let original = std::fs::read(fixture(fixture_name)).expect("image read error");
        assert_eq!(
            dumped.len(),
            original.len(),
            "length mismatch for {}",
            fixture_name
        );
        for (i, (&d, &o)) in dumped.iter().zip(original.iter()).enumerate() {
            if d != o {
                panic!(
                    "{4}: first mismatch at byte {0} ({0:#x}): dumped={1:#04x}, original={2:#04x}\n  region: {3}",
                    i,
                    d,
                    o,
                    if i < IMAGE_HEADER_BYTES {
                        "header"
                    } else if i < IMAGE_HEADER_BYTES
                        + (mem.last_segment() as usize * HEAP_SEGMENT_SIZE
                            + mem.limit_in_last_page() as usize)
                            * 2
                    {
                        "heap"
                    } else if i < (i.div_ceil(IMAGE_PAGE_SIZE_BYTES) * IMAGE_PAGE_SIZE_BYTES) {
                        "gap"
                    } else {
                        "OT"
                    },
                    fixture_name
                );
            }
        }
    }

    #[test]
    fn dump_roundtrips_xerox_v2_snapshot() {
        assert_dump_roundtrip("snapshot.im");
    }

    #[test]
    fn dump_roundtrips_be_fixture() {
        assert_dump_roundtrip("parse_be.im");
    }

    #[test]
    fn dump_roundtrips_le_fixture() {
        assert_dump_roundtrip("parse_le.im");
    }
}

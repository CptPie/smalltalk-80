use std::path::PathBuf;

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
struct ImageHeader {
    last_segment: u16,
    limit_in_last_page: u16,
    object_table_length: u32,
    image_type: u16,
}

impl Image {
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

        if big_endian {
            last_segment = u16::from_be_bytes([data[0], data[1]]); // word 0
            limit_in_last_page = u16::from_be_bytes([data[2], data[3]]); // word 1
            image_type = u16::from_be_bytes([data[8], data[9]]); // word 4
        } else {
            object_table_length = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            last_segment = u16::from_le_bytes([data[0], data[1]]); // word 0
            limit_in_last_page = u16::from_le_bytes([data[2], data[3]]); // word 1
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
}

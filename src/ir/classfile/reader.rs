use std::ops::Range;

use crate::Error;

pub(super) struct Utf8Entries {
    strings: Vec<Option<Range<usize>>>,
    classes: Vec<Option<u16>>,
}

pub(crate) fn class_header_bytes(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut reader = ClassReader::new(bytes);
    reader.skip(8)?;
    let constant_pool_count = usize::from(reader.read_u16()?);
    let names = read_constant_pool(&mut reader, constant_pool_count)?;
    reader.skip(2)?;
    let mut classes = vec![reader.read_u16()?, reader.read_u16()?];
    let interfaces = usize::from(reader.read_u16()?);
    for _ in 0..interfaces {
        classes.push(reader.read_u16()?);
    }
    let required = classes
        .into_iter()
        .filter(|index| *index != 0)
        .map(|index| {
            names
                .classes
                .get(usize::from(index))
                .copied()
                .flatten()
                .ok_or_else(|| invalid_class_file("invalid hierarchy class constant"))
        })
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    let mut header = Vec::new();
    let mut copied = 0;
    for (index, range) in names.strings.iter().enumerate() {
        if let Some(range) = range
            && !required.contains(&(index as u16))
        {
            header.extend_from_slice(&bytes[copied..range.start - 2]);
            header.extend_from_slice(&[0, 0]);
            copied = range.end;
        }
    }
    header.extend_from_slice(&bytes[copied..reader.position()]);
    header.extend_from_slice(&[0; 6]);
    Ok(header)
}

pub(super) fn read_constant_pool(
    reader: &mut ClassReader<'_>,
    constant_pool_count: usize,
) -> Result<Utf8Entries, Error> {
    let mut names = Utf8Entries {
        strings: vec![None; constant_pool_count],
        classes: vec![None; constant_pool_count],
    };
    let mut index = 1;

    while index < constant_pool_count {
        match reader.read_u8()? {
            1 => {
                let length = usize::from(reader.read_u16()?);
                let start = reader.position();
                reader.skip(length)?;
                names.strings[index] = Some(start..reader.position());
            }
            3 | 4 => reader.skip(4)?,
            5 | 6 => {
                reader.skip(8)?;
                index += 1;
            }
            7 => names.classes[index] = Some(reader.read_u16()?),
            8 | 16 | 19 | 20 => reader.skip(2)?,
            9 | 10 | 11 | 12 | 17 | 18 => reader.skip(4)?,
            15 => reader.skip(3)?,
            tag => {
                return Err(invalid_class_file(&format!(
                    "unknown constant-pool tag {tag}"
                )));
            }
        }
        index += 1;
    }

    Ok(names)
}

pub(super) fn attribute_name<'a>(
    bytes: &'a [u8],
    names: &'a Utf8Entries,
    index: u16,
) -> Option<&'a [u8]> {
    names
        .strings
        .get(usize::from(index))?
        .as_ref()
        .map(|range| &bytes[range.clone()])
}

pub(super) fn utf8_name<'a>(
    bytes: &'a [u8],
    names: &'a Utf8Entries,
    index: u16,
) -> Option<&'a str> {
    std::str::from_utf8(attribute_name(bytes, names, index)?).ok()
}

pub(super) fn invalid_class_file(message: &str) -> Error {
    Error::InvalidClassFile {
        message: message.to_owned(),
    }
}

pub(super) struct ClassReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ClassReader<'a> {
    pub(super) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub(super) const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub(super) const fn position(&self) -> usize {
        self.position
    }

    pub(super) fn read_u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn read_u16(&mut self) -> Result<u16, Error> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    pub(super) fn read_u32(&mut self) -> Result<u32, Error> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(super) fn skip(&mut self, length: usize) -> Result<(), Error> {
        let _ = self.take(length)?;
        Ok(())
    }

    pub(super) fn take(&mut self, length: usize) -> Result<&'a [u8], Error> {
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| invalid_class_file("unexpected end of class file"))?;
        let bytes = &self.bytes[self.position..end];
        self.position = end;
        Ok(bytes)
    }
}

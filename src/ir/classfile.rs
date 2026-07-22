use std::ops::Range;

use crate::Error;

const CODE_ATTRIBUTE: &[u8] = b"Code";
const STACK_MAP_TABLE_ATTRIBUTE: &[u8] = b"StackMapTable";

pub(crate) fn strip_stack_map_tables(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut reader = ClassReader::new(bytes);
    reader.skip(4)?;
    reader.skip(2)?;
    reader.skip(2)?;

    let constant_pool_count = usize::from(reader.read_u16()?);
    let names = read_constant_pool(&mut reader, constant_pool_count)?;
    let mut output = bytes[..reader.position()].to_vec();

    copy_bytes(&mut reader, &mut output, 6)?;
    let interface_count = copy_u16(&mut reader, &mut output)?;
    copy_bytes(
        &mut reader,
        &mut output,
        usize::from(interface_count).saturating_mul(2),
    )?;

    let field_count = copy_u16(&mut reader, &mut output)?;
    for _ in 0..field_count {
        copy_member(&mut reader, &mut output)?;
    }

    let method_count = copy_u16(&mut reader, &mut output)?;
    for _ in 0..method_count {
        sanitize_method(&mut reader, &mut output, &names)?;
    }

    let class_attribute_count = copy_u16(&mut reader, &mut output)?;
    for _ in 0..class_attribute_count {
        copy_attribute(&mut reader, &mut output)?;
    }

    if reader.position() != bytes.len() {
        return Err(invalid_class_file("trailing data after class attributes"));
    }

    Ok(output)
}

fn read_constant_pool(
    reader: &mut ClassReader<'_>,
    constant_pool_count: usize,
) -> Result<Vec<Option<Range<usize>>>, Error> {
    let mut names = vec![None; constant_pool_count];
    let mut index = 1;

    while index < constant_pool_count {
        match reader.read_u8()? {
            1 => {
                let length = usize::from(reader.read_u16()?);
                let start = reader.position();
                reader.skip(length)?;
                names[index] = Some(start..reader.position());
            }
            3 | 4 => reader.skip(4)?,
            5 | 6 => {
                reader.skip(8)?;
                index += 1;
            }
            7 | 8 | 16 | 19 | 20 => reader.skip(2)?,
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

fn copy_member(reader: &mut ClassReader<'_>, output: &mut Vec<u8>) -> Result<(), Error> {
    copy_bytes(reader, output, 6)?;
    let attribute_count = copy_u16(reader, output)?;
    for _ in 0..attribute_count {
        copy_attribute(reader, output)?;
    }
    Ok(())
}

fn sanitize_method(
    reader: &mut ClassReader<'_>,
    output: &mut Vec<u8>,
    names: &[Option<Range<usize>>],
) -> Result<(), Error> {
    copy_bytes(reader, output, 6)?;
    let attribute_count = copy_u16(reader, output)?;

    for _ in 0..attribute_count {
        let attribute_start = reader.position();
        let name_index = reader.read_u16()?;
        let length = reader.read_u32()?;
        let body = reader.take(usize::try_from(length).map_err(|_| {
            invalid_class_file("method attribute length does not fit the current platform")
        })?)?;

        if attribute_name(reader.bytes(), names, name_index) == Some(CODE_ATTRIBUTE) {
            let sanitized = sanitize_code_attribute(body, reader.bytes(), names)?;
            write_u16(output, name_index);
            write_u32(
                output,
                u32::try_from(sanitized.len())
                    .map_err(|_| invalid_class_file("sanitized Code attribute is too large"))?,
            );
            output.extend_from_slice(&sanitized);
        } else {
            output.extend_from_slice(&reader.bytes()[attribute_start..reader.position()]);
        }
    }

    Ok(())
}

fn sanitize_code_attribute(
    body: &[u8],
    class_bytes: &[u8],
    names: &[Option<Range<usize>>],
) -> Result<Vec<u8>, Error> {
    let mut reader = ClassReader::new(body);
    let mut output = Vec::with_capacity(body.len());

    copy_bytes(&mut reader, &mut output, 4)?;
    let code_length = usize::try_from(reader.read_u32()?)
        .map_err(|_| invalid_class_file("Code length does not fit the current platform"))?;
    write_u32(
        &mut output,
        u32::try_from(code_length)
            .map_err(|_| invalid_class_file("Code attribute is too large"))?,
    );
    copy_bytes(&mut reader, &mut output, code_length)?;

    let exception_count = copy_u16(&mut reader, &mut output)?;
    copy_bytes(
        &mut reader,
        &mut output,
        usize::from(exception_count).saturating_mul(8),
    )?;

    let attribute_count = reader.read_u16()?;
    let mut nested_attributes = Vec::new();
    let mut kept_attribute_count = 0_u16;
    for _ in 0..attribute_count {
        let attribute_start = reader.position();
        let name_index = reader.read_u16()?;
        let length = reader.read_u32()?;
        reader.skip(usize::try_from(length).map_err(|_| {
            invalid_class_file("Code attribute length does not fit the platform")
        })?)?;

        if attribute_name(class_bytes, names, name_index) != Some(STACK_MAP_TABLE_ATTRIBUTE) {
            nested_attributes
                .extend_from_slice(&reader.bytes()[attribute_start..reader.position()]);
            kept_attribute_count += 1;
        }
    }

    if reader.position() != body.len() {
        return Err(invalid_class_file("trailing data in Code attribute"));
    }

    write_u16(&mut output, kept_attribute_count);
    output.extend_from_slice(&nested_attributes);
    Ok(output)
}

fn copy_attribute(reader: &mut ClassReader<'_>, output: &mut Vec<u8>) -> Result<(), Error> {
    let start = reader.position();
    reader.skip(2)?;
    let length = reader.read_u32()?;
    reader.skip(
        usize::try_from(length).map_err(|_| {
            invalid_class_file("attribute length does not fit the current platform")
        })?,
    )?;
    output.extend_from_slice(&reader.bytes()[start..reader.position()]);
    Ok(())
}

fn attribute_name<'a>(
    bytes: &'a [u8],
    names: &'a [Option<Range<usize>>],
    index: u16,
) -> Option<&'a [u8]> {
    names
        .get(usize::from(index))?
        .as_ref()
        .map(|range| &bytes[range.clone()])
}

fn copy_u16(reader: &mut ClassReader<'_>, output: &mut Vec<u8>) -> Result<u16, Error> {
    let value = reader.read_u16()?;
    write_u16(output, value);
    Ok(value)
}

fn copy_bytes(
    reader: &mut ClassReader<'_>,
    output: &mut Vec<u8>,
    length: usize,
) -> Result<(), Error> {
    output.extend_from_slice(reader.take(length)?);
    Ok(())
}

fn write_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn write_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn invalid_class_file(message: &str) -> Error {
    Error::InvalidClassFile {
        message: message.to_owned(),
    }
}

struct ClassReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ClassReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    const fn position(&self) -> usize {
        self.position
    }

    fn read_u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16, Error> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, Error> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn skip(&mut self, length: usize) -> Result<(), Error> {
        let _ = self.take(length)?;
        Ok(())
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], Error> {
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

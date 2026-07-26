use crate::Error;

use super::reader::{
    ClassReader, Utf8Entries, attribute_name, invalid_class_file, read_constant_pool,
};

const CODE_ATTRIBUTE: &[u8] = b"Code";
const STACK_MAP_TABLE_ATTRIBUTE: &[u8] = b"StackMapTable";

pub(crate) fn strip_stack_map_tables(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut reader = ClassReader::new(bytes);
    reader.skip(8)?;
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

    for _ in 0..copy_u16(&mut reader, &mut output)? {
        copy_member(&mut reader, &mut output)?;
    }
    for _ in 0..copy_u16(&mut reader, &mut output)? {
        sanitize_method(&mut reader, &mut output, &names)?;
    }
    for _ in 0..copy_u16(&mut reader, &mut output)? {
        copy_attribute(&mut reader, &mut output)?;
    }

    (reader.position() == bytes.len())
        .then_some(output)
        .ok_or_else(|| invalid_class_file("trailing data after class attributes"))
}

fn copy_member(reader: &mut ClassReader<'_>, output: &mut Vec<u8>) -> Result<(), Error> {
    copy_bytes(reader, output, 6)?;
    for _ in 0..copy_u16(reader, output)? {
        copy_attribute(reader, output)?;
    }
    Ok(())
}

fn sanitize_method(
    reader: &mut ClassReader<'_>,
    output: &mut Vec<u8>,
    names: &Utf8Entries,
) -> Result<(), Error> {
    copy_bytes(reader, output, 6)?;
    for _ in 0..copy_u16(reader, output)? {
        let attribute_start = reader.position();
        let name = reader.read_u16()?;
        let length = reader.read_u32()?;
        let body = reader.take(usize::try_from(length).map_err(|_| {
            invalid_class_file("method attribute length does not fit the current platform")
        })?)?;
        if attribute_name(reader.bytes(), names, name) == Some(CODE_ATTRIBUTE) {
            let sanitized = sanitize_code_attribute(body, reader.bytes(), names)?;
            write_u16(output, name);
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
    bytes: &[u8],
    class_bytes: &[u8],
    names: &Utf8Entries,
) -> Result<Vec<u8>, Error> {
    let mut reader = ClassReader::new(bytes);
    let mut output = Vec::with_capacity(bytes.len());
    copy_bytes(&mut reader, &mut output, 4)?;
    let code_length = usize::try_from(reader.read_u32()?).map_err(|_| {
        invalid_class_file("Code attribute length does not fit the current platform")
    })?;
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

    let mut nested_attributes = Vec::new();
    let mut kept = 0_u16;
    for _ in 0..reader.read_u16()? {
        let attribute_start = reader.position();
        let name = reader.read_u16()?;
        let length = reader.read_u32()?;
        reader.skip(usize::try_from(length).map_err(|_| {
            invalid_class_file("Code attribute length does not fit the current platform")
        })?)?;
        if attribute_name(class_bytes, names, name) != Some(STACK_MAP_TABLE_ATTRIBUTE) {
            nested_attributes
                .extend_from_slice(&reader.bytes()[attribute_start..reader.position()]);
            kept += 1;
        }
    }

    if reader.position() != bytes.len() {
        return Err(invalid_class_file("trailing data in Code attribute"));
    }
    write_u16(&mut output, kept);
    output.extend_from_slice(&nested_attributes);
    Ok(output)
}

fn copy_attribute(reader: &mut ClassReader<'_>, output: &mut Vec<u8>) -> Result<(), Error> {
    let start = reader.position();
    reader.skip(2)?;
    let length = usize::try_from(reader.read_u32()?)
        .map_err(|_| invalid_class_file("attribute length does not fit the current platform"))?;
    reader.skip(length)?;
    output.extend_from_slice(&reader.bytes()[start..reader.position()]);
    Ok(())
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

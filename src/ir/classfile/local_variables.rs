use std::collections::BTreeMap;

use crate::{Error, IntegralTypeSet, TypeDescriptor};

use super::reader::{
    ClassReader, Utf8Entries, attribute_name, invalid_class_file, read_constant_pool, utf8_name,
};

const CODE_ATTRIBUTE: &[u8] = b"Code";
const LOCAL_VARIABLE_TABLE_ATTRIBUTE: &[u8] = b"LocalVariableTable";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalVariableIntegralHint {
    pub(crate) start_offset: u16,
    pub(crate) end_offset: u16,
    pub(crate) local: u16,
    pub(crate) types: IntegralTypeSet,
}

pub(crate) fn local_variable_integral_hints(
    bytes: &[u8],
) -> BTreeMap<(String, String), Vec<LocalVariableIntegralHint>> {
    parse_local_variable_integral_hints(bytes).unwrap_or_default()
}

fn parse_local_variable_integral_hints(
    bytes: &[u8],
) -> Result<BTreeMap<(String, String), Vec<LocalVariableIntegralHint>>, Error> {
    let mut reader = ClassReader::new(bytes);
    reader.skip(8)?;
    let constant_pool_count = usize::from(reader.read_u16()?);
    let names = read_constant_pool(&mut reader, constant_pool_count)?;
    reader.skip(6)?;
    let interface_count = usize::from(reader.read_u16()?);
    reader.skip(interface_count.saturating_mul(2))?;

    for _ in 0..reader.read_u16()? {
        skip_member(&mut reader)?;
    }

    let mut hints = BTreeMap::new();
    for _ in 0..reader.read_u16()? {
        reader.skip(2)?;
        let method_name = reader.read_u16()?;
        let method_descriptor = reader.read_u16()?;
        let mut method_hints = Vec::new();
        for _ in 0..reader.read_u16()? {
            let attribute = reader.read_u16()?;
            let length = usize::try_from(reader.read_u32()?).map_err(|_| {
                invalid_class_file("method attribute length does not fit the current platform")
            })?;
            let body = reader.take(length)?;
            if attribute_name(bytes, &names, attribute) == Some(CODE_ATTRIBUTE)
                && let Ok(entries) = parse_code_hints(body, bytes, &names)
            {
                method_hints.extend(entries);
            }
        }

        if !method_hints.is_empty()
            && let (Some(name), Some(descriptor)) = (
                utf8_name(bytes, &names, method_name),
                utf8_name(bytes, &names, method_descriptor),
            )
        {
            hints.insert((name.to_owned(), descriptor.to_owned()), method_hints);
        }
    }

    Ok(hints)
}

fn skip_member(reader: &mut ClassReader<'_>) -> Result<(), Error> {
    reader.skip(6)?;
    for _ in 0..reader.read_u16()? {
        reader.skip(2)?;
        let length = usize::try_from(reader.read_u32()?).map_err(|_| {
            invalid_class_file("member attribute length does not fit the current platform")
        })?;
        reader.skip(length)?;
    }
    Ok(())
}

fn parse_code_hints(
    bytes: &[u8],
    class_bytes: &[u8],
    names: &Utf8Entries,
) -> Result<Vec<LocalVariableIntegralHint>, Error> {
    let mut reader = ClassReader::new(bytes);
    reader.skip(4)?;
    let code_length = reader.read_u32()?;
    reader.skip(usize::try_from(code_length).map_err(|_| {
        invalid_class_file("Code attribute length does not fit the current platform")
    })?)?;
    let exception_count = usize::from(reader.read_u16()?);
    reader.skip(exception_count.saturating_mul(8))?;

    let mut hints = Vec::new();
    for _ in 0..reader.read_u16()? {
        let attribute = reader.read_u16()?;
        let length = usize::try_from(reader.read_u32()?).map_err(|_| {
            invalid_class_file("Code attribute length does not fit the current platform")
        })?;
        let body = reader.take(length)?;
        if attribute_name(class_bytes, names, attribute) == Some(LOCAL_VARIABLE_TABLE_ATTRIBUTE)
            && let Ok(entries) = parse_local_variable_table(body, class_bytes, names, code_length)
        {
            hints.extend(entries);
        }
    }
    Ok(hints)
}

fn parse_local_variable_table(
    bytes: &[u8],
    class_bytes: &[u8],
    names: &Utf8Entries,
    code_length: u32,
) -> Result<Vec<LocalVariableIntegralHint>, Error> {
    let mut reader = ClassReader::new(bytes);
    let mut hints = Vec::new();
    for _ in 0..reader.read_u16()? {
        let start_offset = reader.read_u16()?;
        let length = reader.read_u16()?;
        let _name = reader.read_u16()?;
        let descriptor = reader.read_u16()?;
        let local = reader.read_u16()?;
        let Some(end_offset) = start_offset.checked_add(length) else {
            continue;
        };
        if u32::from(end_offset) > code_length {
            continue;
        }
        let Some(descriptor) = utf8_name(class_bytes, names, descriptor) else {
            continue;
        };
        let Ok(TypeDescriptor::Primitive(primitive)) = TypeDescriptor::parse(descriptor) else {
            continue;
        };
        let Some(types) = IntegralTypeSet::from_primitive(primitive) else {
            continue;
        };
        hints.push(LocalVariableIntegralHint {
            start_offset,
            end_offset,
            local,
            types,
        });
    }

    (reader.position() == bytes.len())
        .then_some(hints)
        .ok_or_else(|| invalid_class_file("trailing data in LocalVariableTable attribute"))
}

use std::collections::HashMap;

use crate::ir::{ClassIr, InstructionIr, InstructionOperandIr, MemberRefIr, MethodIr};
use crate::summary::value_type_matches_descriptor;
use crate::{
    ClassName, FieldSummaries, FieldSummaryResolver, InferredType, MethodInference, TypeDescriptor,
    TypeHierarchy,
};

pub(super) struct StaticFieldResolver<'a> {
    external: Option<&'a dyn FieldSummaryResolver>,
    local: &'a FieldSummaries,
}

impl StaticFieldResolver<'_> {
    pub(super) fn new<'a>(
        external: Option<&'a dyn FieldSummaryResolver>,
        local: &'a FieldSummaries,
    ) -> StaticFieldResolver<'a> {
        StaticFieldResolver { external, local }
    }
}

impl FieldSummaryResolver for StaticFieldResolver<'_> {
    fn value_type(
        &self,
        owner: &ClassName,
        name: &str,
        descriptor: &TypeDescriptor,
    ) -> Option<InferredType> {
        self.external
            .and_then(|resolver| resolver.value_type(owner, name, descriptor))
            .or_else(|| self.local.value_type(owner, name, descriptor))
    }
}

pub(super) fn update_local_static_field_summaries(
    class: &ClassIr,
    method: &MethodIr,
    inference: &MethodInference,
    fields: &mut FieldSummaries,
    hierarchy: Option<&dyn TypeHierarchy>,
) -> Vec<FieldKey> {
    if method.name != "<clinit>" || !inference.analysis_complete() {
        return Vec::new();
    }

    let observations = inference
        .instructions()
        .iter()
        .map(|instruction| (instruction.bytecode_offset(), instruction))
        .collect::<HashMap<_, _>>();
    let mut discovered = HashMap::new();

    for instruction in method
        .instructions
        .iter()
        .filter(|instruction| instruction.opcode == 0xb3)
    {
        let Some(key) = local_static_field_key(class, instruction) else {
            continue;
        };
        let Some(value_type) = observations
            .get(&instruction.offset)
            .and_then(|instruction| instruction.stack_before().last())
        else {
            continue;
        };
        if !value_type_matches_descriptor(&key.descriptor, value_type) {
            continue;
        }
        discovered
            .entry(key)
            .and_modify(|current: &mut InferredType| {
                *current = current.join_with_hierarchy(value_type, hierarchy);
            })
            .or_insert_with(|| value_type.clone());
    }

    let mut changed = Vec::new();
    for (key, value_type) in discovered {
        if fields
            .value_type(&class.name, &key.name, &key.descriptor)
            .as_ref()
            == Some(&value_type)
        {
            continue;
        }
        fields.insert_value_type(
            class.name.clone(),
            key.name.clone(),
            key.descriptor.clone(),
            value_type,
        );
        changed.push(key);
    }
    changed
}

pub(super) fn local_field_readers(class: &ClassIr) -> HashMap<FieldKey, Vec<usize>> {
    let mut readers = HashMap::<FieldKey, Vec<usize>>::new();
    for (method_index, method) in class.methods.iter().enumerate() {
        for instruction in method
            .instructions
            .iter()
            .filter(|instruction| instruction.opcode == 0xb2)
        {
            let Some(key) = local_static_field_key(class, instruction) else {
                continue;
            };
            readers.entry(key).or_default().push(method_index);
        }
    }
    for field_readers in readers.values_mut() {
        field_readers.sort_unstable();
        field_readers.dedup();
    }
    readers
}

fn local_static_field_key(class: &ClassIr, instruction: &InstructionIr) -> Option<FieldKey> {
    let InstructionOperandIr::Member(MemberRefIr::Resolved {
        owner,
        name,
        descriptor,
    }) = &instruction.operand
    else {
        return None;
    };
    if owner != &class.name {
        return None;
    }
    Some(FieldKey {
        name: name.clone(),
        descriptor: TypeDescriptor::parse(descriptor).ok()?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct FieldKey {
    pub(super) name: String,
    pub(super) descriptor: TypeDescriptor,
}

use std::collections::{HashMap, HashSet};

use crate::ir::{ClassIr, InstructionIr, InstructionOperandIr, MemberRefIr, MethodIr};
use crate::{ClassName, MethodDescriptor, MethodInvocationKind};
use rust_asm::opcodes as op;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct MethodKey {
    pub(super) name: String,
    pub(super) descriptor: MethodDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct BatchMethodKey {
    pub(super) owner: ClassName,
    pub(super) method: MethodKey,
}

impl MethodKey {
    pub(super) fn from_method(method: &MethodIr) -> Self {
        Self {
            name: method.name.clone(),
            descriptor: method.descriptor.clone(),
        }
    }
}

pub(super) struct LocalMethodCalls<'a> {
    pub(super) owner: &'a ClassName,
    pub(super) class_is_final: bool,
    pub(super) methods: &'a [MethodIr],
    pub(super) method_indices: &'a HashMap<MethodKey, usize>,
}

pub(super) fn local_method_indices(class: &ClassIr) -> HashMap<MethodKey, usize> {
    class
        .methods
        .iter()
        .enumerate()
        .map(|(index, method)| (MethodKey::from_method(method), index))
        .collect()
}

pub(super) fn local_summary_callers(
    class: &ClassIr,
    local_calls: &LocalMethodCalls<'_>,
) -> Vec<Vec<usize>> {
    let mut callers = vec![Vec::new(); class.methods.len()];

    for (caller_index, method) in class.methods.iter().enumerate() {
        for instruction in method.instructions.iter().filter(|instruction| {
            matches!(instruction.opcode, op::INVOKEVIRTUAL..=op::INVOKESTATIC)
        }) {
            let Some((owner, name, descriptor)) = resolved_method_reference(instruction) else {
                continue;
            };
            if owner != &class.name {
                continue;
            }
            let Ok(descriptor) = MethodDescriptor::parse(descriptor) else {
                continue;
            };
            let key = MethodKey {
                name: name.to_owned(),
                descriptor,
            };
            let Some(target_index) = local_calls.method_indices.get(&key) else {
                continue;
            };
            let Some(invocation_kind) = MethodInvocationKind::from_opcode(instruction.opcode)
            else {
                continue;
            };
            if invocation_kind != MethodInvocationKind::Virtual
                && !local_call_is_deterministic(
                    local_calls,
                    owner,
                    name,
                    &key.descriptor,
                    invocation_kind,
                    false,
                )
            {
                continue;
            }
            callers[*target_index].push(caller_index);
        }
    }

    for callers in &mut callers {
        callers.sort_unstable();
        callers.dedup();
    }
    callers
}

pub(super) fn local_call_is_deterministic(
    local_calls: &LocalMethodCalls<'_>,
    owner: &ClassName,
    name: &str,
    descriptor: &MethodDescriptor,
    invocation_kind: MethodInvocationKind,
    receiver_is_exact_allocation: bool,
) -> bool {
    if owner != local_calls.owner {
        return false;
    }
    match invocation_kind {
        MethodInvocationKind::Static | MethodInvocationKind::Special => true,
        MethodInvocationKind::Virtual => {
            if local_calls.class_is_final || receiver_is_exact_allocation {
                return true;
            }
            let key = MethodKey {
                name: name.to_owned(),
                descriptor: descriptor.clone(),
            };
            local_calls
                .method_indices
                .get(&key)
                .is_some_and(|index| local_calls.methods[*index].access_flags & 0x0010 != 0)
        }
        MethodInvocationKind::Interface => false,
    }
}

pub(super) fn resolved_method_reference(
    instruction: &InstructionIr,
) -> Option<(&ClassName, &str, &str)> {
    let member = match &instruction.operand {
        InstructionOperandIr::Member(member) => member,
        InstructionOperandIr::InvokeInterface { method, .. } => method,
        _ => return None,
    };
    let MemberRefIr::Resolved {
        owner,
        name,
        descriptor,
    } = member
    else {
        return None;
    };
    Some((owner, name, descriptor))
}

pub(super) struct BatchCallTargets {
    final_classes: HashSet<ClassName>,
    final_methods: HashSet<BatchMethodKey>,
}

impl BatchCallTargets {
    pub(super) fn from_classes(classes: &[ClassIr]) -> Self {
        let mut final_classes = HashSet::new();
        let mut final_methods = HashSet::new();
        for class in classes {
            if class.access_flags & 0x0010 != 0 {
                final_classes.insert(class.name.clone());
            }
            for method in &class.methods {
                if method.access_flags & 0x0010 != 0 {
                    final_methods.insert(BatchMethodKey {
                        owner: class.name.clone(),
                        method: MethodKey::from_method(method),
                    });
                }
            }
        }
        Self {
            final_classes,
            final_methods,
        }
    }

    pub(super) fn is_deterministic(
        &self,
        owner: &ClassName,
        name: &str,
        descriptor: &MethodDescriptor,
        invocation_kind: MethodInvocationKind,
        receiver_is_exact_allocation: bool,
    ) -> bool {
        match invocation_kind {
            MethodInvocationKind::Static | MethodInvocationKind::Special => true,
            MethodInvocationKind::Virtual => {
                receiver_is_exact_allocation
                    || self.final_classes.contains(owner)
                    || self.final_methods.contains(&BatchMethodKey {
                        owner: owner.clone(),
                        method: MethodKey {
                            name: name.to_owned(),
                            descriptor: descriptor.clone(),
                        },
                    })
            }
            MethodInvocationKind::Interface => false,
        }
    }
}

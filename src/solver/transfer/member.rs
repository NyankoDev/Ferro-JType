use crate::ir::{InstructionIr, InstructionOperandIr, MemberRefIr, MethodIr};
use crate::solver::frame::{Frame, FrameValue, ValueOrigin, inferred_from_descriptor};
use crate::summary::{FieldSummaryResolver, MethodSummaryResolver, value_type_matches_descriptor};
use crate::{
    Diagnostic, InferredType, MethodDescriptor, MethodInvocationKind, ReferenceType, ReturnType,
    TypeDescriptor,
};
use rust_asm::opcodes as op;

use super::{discard, pop_value, unsupported};

pub(super) fn field_get(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
    has_receiver: bool,
    field_summaries: Option<&dyn FieldSummaryResolver>,
) {
    if has_receiver {
        discard(frame, method, instruction, diagnostics);
    }
    let field_summaries = (!has_receiver).then_some(field_summaries).flatten();
    frame.push(field_type(
        instruction,
        method,
        diagnostics,
        field_summaries,
    ));
}

pub(super) fn field_put(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
    has_receiver: bool,
) {
    discard(frame, method, instruction, diagnostics);
    if has_receiver {
        discard(frame, method, instruction, diagnostics);
    }
}

pub(super) fn invoke_member(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
    method_summaries: Option<&dyn MethodSummaryResolver>,
) {
    let Some((descriptor, member)) = method_call_descriptor(instruction, method, diagnostics)
    else {
        frame.clear_stack();
        return;
    };

    let mut arguments = descriptor
        .parameters()
        .iter()
        .map(|_| pop_value(frame, method, instruction, diagnostics))
        .collect::<Vec<_>>();
    arguments.reverse();

    let receiver = (instruction.opcode != op::INVOKESTATIC)
        .then(|| pop_value(frame, method, instruction, diagnostics));
    if let Some(MemberRefIr::ArrayMethod { owner, name, .. }) = member {
        let return_type = (instruction.opcode == op::INVOKEVIRTUAL
            && name == "clone"
            && descriptor.parameters().is_empty()
            && descriptor.return_type()
                == &ReturnType::Type(TypeDescriptor::Reference(
                    crate::ClassName::java_lang_object(),
                )))
            .then(|| InferredType::Reference(ReferenceType::Array(owner.clone())));
        push_return_type(&descriptor, return_type, frame);
        return;
    }
    if let (Some(MemberRefIr::Resolved { name, owner, .. }), Some(receiver)) = (member, &receiver)
        && name == "<init>"
    {
        match &receiver.value {
            InferredType::Uninitialized {
                allocation_offset, ..
            } => frame.replace_uninitialized(*allocation_offset, owner.clone()),
            InferredType::UninitializedThis { class_name } => {
                frame.replace_uninitialized_this(class_name.clone())
            }
            _ => {}
        }
    }

    let invocation_kind = MethodInvocationKind::from_opcode(instruction.opcode);
    let receiver_is_exact_allocation =
        receiver_is_exact_allocation(method, member, receiver.as_ref());
    let summary_return_type = invocation_kind.and_then(|invocation_kind| {
        member.and_then(|member| {
            resolve_method_summary(
                member,
                &descriptor,
                method_summaries,
                invocation_kind,
                receiver_is_exact_allocation,
            )
        })
    });
    let parameter_return_type = invocation_kind.and_then(|invocation_kind| {
        member.and_then(|member| {
            resolve_returned_parameter(
                member,
                &descriptor,
                method_summaries,
                invocation_kind,
                &arguments,
                receiver_is_exact_allocation,
                summary_return_type.as_ref(),
            )
        })
    });
    if let Some(value) = parameter_return_type {
        frame.push_value(value);
    } else {
        push_return_type(&descriptor, summary_return_type, frame);
    }
}

pub(super) fn invoke_dynamic(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let InstructionOperandIr::InvokeDynamic { descriptor, .. } = &instruction.operand else {
        return;
    };
    let Some(descriptor) = descriptor
        .as_deref()
        .and_then(|descriptor| MethodDescriptor::parse(descriptor).ok())
    else {
        unsupported(method, instruction, diagnostics);
        frame.clear_stack();
        return;
    };

    for _ in descriptor.parameters() {
        discard(frame, method, instruction, diagnostics);
    }
    push_return_type(&descriptor, None, frame);
}

fn method_call_descriptor<'a>(
    instruction: &'a InstructionIr,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(MethodDescriptor, Option<&'a MemberRefIr>)> {
    let member = match &instruction.operand {
        InstructionOperandIr::Member(member) => member,
        InstructionOperandIr::InvokeInterface { method, .. } => method,
        _ => return None,
    };
    let (MemberRefIr::Resolved { descriptor, .. } | MemberRefIr::ArrayMethod { descriptor, .. }) =
        member
    else {
        unsupported(method, instruction, diagnostics);
        return None;
    };

    match MethodDescriptor::parse(descriptor) {
        Ok(descriptor) => Some((descriptor, Some(member))),
        Err(_) => {
            unsupported(method, instruction, diagnostics);
            None
        }
    }
}

fn resolve_method_summary(
    member: &MemberRefIr,
    descriptor: &MethodDescriptor,
    method_summaries: Option<&dyn MethodSummaryResolver>,
    invocation_kind: MethodInvocationKind,
    receiver_is_exact_allocation: bool,
) -> Option<InferredType> {
    let MemberRefIr::Resolved { owner, name, .. } = member else {
        return None;
    };
    let return_type = method_summaries?.return_type_for_call(
        owner,
        name,
        descriptor,
        invocation_kind,
        receiver_is_exact_allocation,
    )?;
    method_summary_is_compatible(descriptor, &return_type).then_some(return_type)
}

fn resolve_returned_parameter(
    member: &MemberRefIr,
    descriptor: &MethodDescriptor,
    method_summaries: Option<&dyn MethodSummaryResolver>,
    invocation_kind: MethodInvocationKind,
    arguments: &[FrameValue],
    receiver_is_exact_allocation: bool,
    summary_return_type: Option<&InferredType>,
) -> Option<FrameValue> {
    let MemberRefIr::Resolved { owner, name, .. } = member else {
        return None;
    };
    let parameter_index = method_summaries?.returned_parameter_index_for_call(
        owner,
        name,
        descriptor,
        invocation_kind,
        receiver_is_exact_allocation,
    )?;
    let parameter = descriptor.parameters().get(parameter_index)?;
    let mut value = arguments.get(parameter_index)?.clone();
    if let Some(summary) = summary_return_type
        && summary != &inferred_from_descriptor(parameter)
        && !matches!(value.value, InferredType::Reference(ReferenceType::Null))
    {
        value.value = summary.clone();
    }
    method_summary_is_compatible(descriptor, &value.value).then_some(value)
}

fn method_summary_is_compatible(descriptor: &MethodDescriptor, return_type: &InferredType) -> bool {
    match descriptor.return_type() {
        ReturnType::Void => false,
        ReturnType::Type(descriptor) => value_type_matches_descriptor(descriptor, return_type),
    }
}

fn push_return_type(
    descriptor: &MethodDescriptor,
    summary_return_type: Option<InferredType>,
    frame: &mut Frame,
) {
    if let ReturnType::Type(return_type) = descriptor.return_type() {
        frame.push(summary_return_type.unwrap_or_else(|| inferred_from_descriptor(return_type)));
    }
}

fn field_type(
    instruction: &InstructionIr,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
    field_summaries: Option<&dyn FieldSummaryResolver>,
) -> InferredType {
    let InstructionOperandIr::Member(MemberRefIr::Resolved {
        owner,
        name,
        descriptor,
    }) = &instruction.operand
    else {
        unsupported(method, instruction, diagnostics);
        return InferredType::Reference(ReferenceType::Unknown);
    };

    TypeDescriptor::parse(descriptor)
        .map(|descriptor| {
            field_summaries
                .and_then(|resolver| resolver.value_type(owner, name, &descriptor))
                .filter(|value_type| value_type_matches_descriptor(&descriptor, value_type))
                .unwrap_or_else(|| inferred_from_descriptor(&descriptor))
        })
        .unwrap_or_else(|_| {
            unsupported(method, instruction, diagnostics);
            InferredType::Reference(ReferenceType::Unknown)
        })
}

fn receiver_is_exact_allocation(
    method: &MethodIr,
    member: Option<&MemberRefIr>,
    receiver: Option<&FrameValue>,
) -> bool {
    let Some(MemberRefIr::Resolved { owner, .. }) = member else {
        return false;
    };
    let Some(FrameValue {
        value: InferredType::Reference(ReferenceType::Exact(class_name)),
        local_origin: Some(ValueOrigin::Allocation { offset }),
        ..
    }) = receiver
    else {
        return false;
    };
    if class_name != owner {
        return false;
    }
    let Ok(index) = method
        .instructions
        .binary_search_by_key(offset, |instruction| instruction.offset)
    else {
        return false;
    };
    matches!(&method.instructions[index], InstructionIr {
        opcode: op::NEW,
        operand: InstructionOperandIr::Type { type_name: Some(allocated), .. },
        ..
    } if allocated == owner.as_str())
}

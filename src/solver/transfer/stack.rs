use crate::ir::{InstructionIr, InstructionOperandIr, MemberRefIr, MethodIr};
use crate::solver::frame::{Frame, FrameValue};
use crate::{Diagnostic, DiagnosticKind, DiagnosticLocation, DiagnosticSeverity, InferredType};

pub(super) fn discard(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let _ = pop(frame, method, instruction, diagnostics);
}

pub(super) fn discard_two_slots(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let top = pop(frame, method, instruction, diagnostics);
    if !is_category_two(&top) {
        discard(frame, method, instruction, diagnostics);
    }
}

pub(super) fn pop(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) -> InferredType {
    pop_value(frame, method, instruction, diagnostics).value
}

pub(super) fn pop_value(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) -> FrameValue {
    frame.pop_value().unwrap_or_else(|| {
        diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Warning,
            DiagnosticKind::StackUnderflow,
            location(method, instruction.offset),
            "instruction consumed a value from an empty operand stack",
        ));
        FrameValue::plain(InferredType::Conflict)
    })
}

pub(super) fn duplicate_top(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let value = pop_value(frame, method, instruction, diagnostics);
    frame.push_value(value.clone());
    frame.push_value(value);
}

pub(super) fn duplicate_x1(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let first = pop_value(frame, method, instruction, diagnostics);
    let second = pop_value(frame, method, instruction, diagnostics);
    frame.push_value(first.clone());
    frame.push_value(second);
    frame.push_value(first);
}

pub(super) fn duplicate_x2(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let first = pop_value(frame, method, instruction, diagnostics);
    let second = pop_value(frame, method, instruction, diagnostics);
    if is_category_two(&second.value) {
        frame.push_value(first.clone());
        frame.push_value(second);
        frame.push_value(first);
    } else {
        let third = pop_value(frame, method, instruction, diagnostics);
        frame.push_value(first.clone());
        frame.push_value(third);
        frame.push_value(second);
        frame.push_value(first);
    }
}

pub(super) fn duplicate_two(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let first = pop_value(frame, method, instruction, diagnostics);
    if is_category_two(&first.value) {
        frame.push_value(first.clone());
        frame.push_value(first);
    } else {
        let second = pop_value(frame, method, instruction, diagnostics);
        frame.push_value(second.clone());
        frame.push_value(first.clone());
        frame.push_value(second);
        frame.push_value(first);
    }
}

pub(super) fn duplicate_two_x1(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let first = pop_value(frame, method, instruction, diagnostics);
    let second = pop_value(frame, method, instruction, diagnostics);
    if is_category_two(&first.value) {
        frame.push_value(first.clone());
        frame.push_value(second);
        frame.push_value(first);
    } else {
        let third = pop_value(frame, method, instruction, diagnostics);
        frame.push_value(second.clone());
        frame.push_value(first.clone());
        frame.push_value(third);
        frame.push_value(second);
        frame.push_value(first);
    }
}

pub(super) fn duplicate_two_x2(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let first = pop_value(frame, method, instruction, diagnostics);
    let second = pop_value(frame, method, instruction, diagnostics);
    if is_category_two(&first.value) && is_category_two(&second.value) {
        frame.push_value(first.clone());
        frame.push_value(second);
        frame.push_value(first);
    } else if is_category_two(&first.value) {
        let third = pop_value(frame, method, instruction, diagnostics);
        frame.push_value(first.clone());
        frame.push_value(third);
        frame.push_value(second);
        frame.push_value(first);
    } else {
        let third = pop_value(frame, method, instruction, diagnostics);
        if is_category_two(&third.value) {
            frame.push_value(second.clone());
            frame.push_value(first.clone());
            frame.push_value(third);
            frame.push_value(second);
            frame.push_value(first);
        } else {
            let fourth = pop_value(frame, method, instruction, diagnostics);
            frame.push_value(second.clone());
            frame.push_value(first.clone());
            frame.push_value(fourth);
            frame.push_value(third);
            frame.push_value(second);
            frame.push_value(first);
        }
    }
}

pub(super) fn swap(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let first = pop_value(frame, method, instruction, diagnostics);
    let second = pop_value(frame, method, instruction, diagnostics);
    frame.push_value(first);
    frame.push_value(second);
}

pub(super) fn unsupported(
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let details = operand_details(&instruction.operand);
    diagnostics.push(Diagnostic::new(
        DiagnosticSeverity::Warning,
        DiagnosticKind::UnsupportedInstruction,
        location(method, instruction.offset),
        format!(
            "opcode 0x{:02x} is not modeled{details}",
            instruction.opcode
        ),
    ));
}

pub(super) fn operand_details(operand: &InstructionOperandIr) -> String {
    match operand {
        InstructionOperandIr::ConstantPool(index) => format!(" (constant-pool index {index})"),
        InstructionOperandIr::Type {
            constant_pool_index,
            ..
        }
        | InstructionOperandIr::MultiArray {
            constant_pool_index,
            ..
        } => format!(" (type constant-pool index {constant_pool_index})"),
        InstructionOperandIr::InvokeInterface { count, .. } => {
            format!(" (invokeinterface argument count {count})")
        }
        InstructionOperandIr::InvokeDynamic {
            constant_pool_index,
            ..
        } => format!(" (invokedynamic constant-pool index {constant_pool_index})"),
        InstructionOperandIr::Increment { amount, .. } => format!(" (increment {amount})"),
        InstructionOperandIr::Member(MemberRefIr::Unresolved {
            constant_pool_index,
        }) => format!(" (unresolved member constant-pool index {constant_pool_index})"),
        _ => String::new(),
    }
}

pub(super) fn location(method: &MethodIr, offset: u16) -> DiagnosticLocation {
    DiagnosticLocation::method(&method.name, &method.descriptor_text).at_offset(offset)
}

pub(super) const fn is_category_two(value: &InferredType) -> bool {
    matches!(value, InferredType::Long | InferredType::Double)
}

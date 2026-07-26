use crate::ir::{ConstantKind, InstructionIr, InstructionOperandIr, MethodIr};
use crate::solver::frame::{Frame, InstanceOfFact, inferred_from_descriptor};
use crate::summary::{FieldSummaryResolver, MethodSummaryResolver};
use crate::{ClassName, Diagnostic, InferredType, IntegralTypeSet, ReferenceType, TypeDescriptor};
use rust_asm::opcodes as op;

mod array;
mod member;
mod stack;

use array::*;
use member::*;
use stack::*;

pub(crate) fn transfer(
    method: &MethodIr,
    instruction: &InstructionIr,
    frame: &mut Frame,
    diagnostics: &mut Vec<Diagnostic>,
    method_summaries: Option<&dyn MethodSummaryResolver>,
    field_summaries: Option<&dyn FieldSummaryResolver>,
) {
    match instruction.opcode {
        op::NOP => {}
        op::ACONST_NULL => frame.push(InferredType::Reference(ReferenceType::Null)),
        op::ICONST_M1..=op::ICONST_5 | op::BIPUSH | op::SIPUSH => {
            frame.push(InferredType::Integral(IntegralTypeSet::ALL))
        }
        op::LCONST_0..=op::LCONST_1 => frame.push(InferredType::Long),
        op::FCONST_0..=op::FCONST_2 => frame.push(InferredType::Float),
        op::DCONST_0..=op::DCONST_1 => frame.push(InferredType::Double),
        op::LDC..=op::LDC2_W => push_constant(instruction, frame),
        op::ILOAD | op::ILOAD_0..=op::ILOAD_3 => {
            load_local(instruction, frame, op::ILOAD, op::ILOAD_0)
        }
        op::LLOAD | op::LLOAD_0..=op::LLOAD_3 => {
            load_local(instruction, frame, op::LLOAD, op::LLOAD_0)
        }
        op::FLOAD | op::FLOAD_0..=op::FLOAD_3 => {
            load_local(instruction, frame, op::FLOAD, op::FLOAD_0)
        }
        op::DLOAD | op::DLOAD_0..=op::DLOAD_3 => {
            load_local(instruction, frame, op::DLOAD, op::DLOAD_0)
        }
        op::ALOAD | op::ALOAD_0..=op::ALOAD_3 => {
            load_local(instruction, frame, op::ALOAD, op::ALOAD_0)
        }
        op::IALOAD => integral_array_load(
            frame,
            IntegralTypeSet::INT,
            method,
            instruction,
            diagnostics,
        ),
        op::BALOAD => integral_array_load(
            frame,
            IntegralTypeSet::BOOLEAN.union(IntegralTypeSet::BYTE),
            method,
            instruction,
            diagnostics,
        ),
        op::CALOAD => integral_array_load(
            frame,
            IntegralTypeSet::CHAR,
            method,
            instruction,
            diagnostics,
        ),
        op::SALOAD => integral_array_load(
            frame,
            IntegralTypeSet::SHORT,
            method,
            instruction,
            diagnostics,
        ),
        op::LALOAD => array_load(frame, InferredType::Long, method, instruction, diagnostics),
        op::FALOAD => array_load(frame, InferredType::Float, method, instruction, diagnostics),
        op::DALOAD => array_load(
            frame,
            InferredType::Double,
            method,
            instruction,
            diagnostics,
        ),
        op::AALOAD => reference_array_load(frame, method, instruction, diagnostics),
        op::ISTORE | op::ISTORE_0..=op::ISTORE_3 => store_local(
            instruction,
            frame,
            op::ISTORE,
            op::ISTORE_0,
            method,
            diagnostics,
        ),
        op::LSTORE | op::LSTORE_0..=op::LSTORE_3 => store_local(
            instruction,
            frame,
            op::LSTORE,
            op::LSTORE_0,
            method,
            diagnostics,
        ),
        op::FSTORE | op::FSTORE_0..=op::FSTORE_3 => store_local(
            instruction,
            frame,
            op::FSTORE,
            op::FSTORE_0,
            method,
            diagnostics,
        ),
        op::DSTORE | op::DSTORE_0..=op::DSTORE_3 => store_local(
            instruction,
            frame,
            op::DSTORE,
            op::DSTORE_0,
            method,
            diagnostics,
        ),
        op::ASTORE | op::ASTORE_0..=op::ASTORE_3 => store_local(
            instruction,
            frame,
            op::ASTORE,
            op::ASTORE_0,
            method,
            diagnostics,
        ),
        op::IASTORE..=op::SASTORE => array_store(frame, method, instruction, diagnostics),
        op::POP => discard(frame, method, instruction, diagnostics),
        op::POP2 => discard_two_slots(frame, method, instruction, diagnostics),
        op::DUP => duplicate_top(frame, method, instruction, diagnostics),
        op::DUP_X1 => duplicate_x1(frame, method, instruction, diagnostics),
        op::DUP_X2 => duplicate_x2(frame, method, instruction, diagnostics),
        op::DUP2 => duplicate_two(frame, method, instruction, diagnostics),
        op::DUP2_X1 => duplicate_two_x1(frame, method, instruction, diagnostics),
        op::DUP2_X2 => duplicate_two_x2(frame, method, instruction, diagnostics),
        op::SWAP => swap(frame, method, instruction, diagnostics),
        op::IADD
        | op::ISUB
        | op::IMUL
        | op::IDIV
        | op::IREM
        | op::ISHL
        | op::ISHR
        | op::IUSHR
        | op::IAND
        | op::IOR
        | op::IXOR => binary(frame, InferredType::Int, method, instruction, diagnostics),
        op::LADD
        | op::LSUB
        | op::LMUL
        | op::LDIV
        | op::LREM
        | op::LSHL
        | op::LSHR
        | op::LUSHR
        | op::LAND
        | op::LOR
        | op::LXOR => binary(frame, InferredType::Long, method, instruction, diagnostics),
        op::FADD | op::FSUB | op::FMUL | op::FDIV | op::FREM => {
            binary(frame, InferredType::Float, method, instruction, diagnostics)
        }
        op::DADD | op::DSUB | op::DMUL | op::DDIV | op::DREM => binary(
            frame,
            InferredType::Double,
            method,
            instruction,
            diagnostics,
        ),
        op::INEG | op::FNEG | op::DNEG => unary(frame, method, instruction, diagnostics),
        op::LNEG => unary(frame, method, instruction, diagnostics),
        op::IINC => increment_local(instruction, frame),
        op::I2L => convert(frame, InferredType::Long, method, instruction, diagnostics),
        op::I2F => convert(frame, InferredType::Float, method, instruction, diagnostics),
        op::I2D => convert(
            frame,
            InferredType::Double,
            method,
            instruction,
            diagnostics,
        ),
        op::L2I | op::F2I | op::D2I => {
            convert(frame, InferredType::Int, method, instruction, diagnostics)
        }
        op::I2B => convert(
            frame,
            InferredType::Integral(IntegralTypeSet::BYTE),
            method,
            instruction,
            diagnostics,
        ),
        op::I2C => convert(
            frame,
            InferredType::Integral(IntegralTypeSet::CHAR),
            method,
            instruction,
            diagnostics,
        ),
        op::I2S => convert(
            frame,
            InferredType::Integral(IntegralTypeSet::SHORT),
            method,
            instruction,
            diagnostics,
        ),
        op::L2F | op::F2L | op::D2L => {
            convert(frame, InferredType::Long, method, instruction, diagnostics)
        }
        op::L2D | op::F2D | op::D2F => {
            convert(frame, InferredType::Float, method, instruction, diagnostics)
        }
        op::LCMP..=op::DCMPG => binary(frame, InferredType::Int, method, instruction, diagnostics),
        op::IFEQ..=op::IFLE | op::IFNULL | op::IFNONNULL => {
            discard(frame, method, instruction, diagnostics)
        }
        op::IF_ICMPEQ..=op::IF_ACMPNE => {
            discard(frame, method, instruction, diagnostics);
            discard(frame, method, instruction, diagnostics);
        }
        op::JSR | op::JSR_W => push_subroutine_return_address(method, instruction, frame),
        op::TABLESWITCH | op::LOOKUPSWITCH => discard(frame, method, instruction, diagnostics),
        op::IRETURN..=op::ARETURN => discard(frame, method, instruction, diagnostics),
        op::RETURN | op::GOTO | op::RET | op::GOTO_W => {}
        op::GETSTATIC => field_get(
            instruction,
            frame,
            method,
            diagnostics,
            false,
            field_summaries,
        ),
        op::PUTSTATIC => field_put(instruction, frame, method, diagnostics, false),
        op::GETFIELD => field_get(
            instruction,
            frame,
            method,
            diagnostics,
            true,
            field_summaries,
        ),
        op::PUTFIELD => field_put(instruction, frame, method, diagnostics, true),
        op::INVOKEVIRTUAL..=op::INVOKEINTERFACE => {
            invoke_member(instruction, frame, method, diagnostics, method_summaries)
        }
        op::INVOKEDYNAMIC => invoke_dynamic(instruction, frame, method, diagnostics),
        op::NEW => allocate_object(instruction, frame),
        op::NEWARRAY => allocate_primitive_array(instruction, frame, method, diagnostics),
        op::ANEWARRAY => allocate_reference_array(instruction, frame, method, diagnostics),
        op::ARRAYLENGTH => {
            discard(frame, method, instruction, diagnostics);
            frame.push(InferredType::Int);
        }
        op::ATHROW => discard(frame, method, instruction, diagnostics),
        op::CHECKCAST => cast_reference(instruction, frame, method, diagnostics),
        op::INSTANCEOF => instance_of(instruction, frame, method, diagnostics),
        op::MONITORENTER | op::MONITOREXIT => discard(frame, method, instruction, diagnostics),
        op::MULTIANEWARRAY => allocate_multi_array(instruction, frame, method, diagnostics),
        op::BREAKPOINT | op::IMPDEP1 | op::IMPDEP2 => unsupported(method, instruction, diagnostics),
        _ => unsupported(method, instruction, diagnostics),
    }
}

fn load_local(instruction: &InstructionIr, frame: &mut Frame, wide_opcode: u8, short_base: u8) {
    let local = local_index(instruction, wide_opcode, short_base).unwrap_or_default();
    frame.push_local(local, instruction.offset);
}

fn store_local(
    instruction: &InstructionIr,
    frame: &mut Frame,
    wide_opcode: u8,
    short_base: u8,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let value = pop_value(frame, method, instruction, diagnostics);
    let local = local_index(instruction, wide_opcode, short_base).unwrap_or_default();
    frame.store_local_value(local, value, instruction.offset);
}

fn local_index(instruction: &InstructionIr, wide_opcode: u8, short_base: u8) -> Option<u16> {
    if instruction.opcode == wide_opcode {
        let InstructionOperandIr::Local(local) = instruction.operand else {
            return None;
        };
        return Some(local);
    }

    instruction
        .opcode
        .checked_sub(short_base)
        .filter(|index| *index < 4)
        .map(u16::from)
}

fn binary(
    frame: &mut Frame,
    result: InferredType,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    discard(frame, method, instruction, diagnostics);
    discard(frame, method, instruction, diagnostics);
    frame.push(result);
}

fn unary(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let value = pop(frame, method, instruction, diagnostics);
    frame.push(value);
}

fn convert(
    frame: &mut Frame,
    result: InferredType,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    discard(frame, method, instruction, diagnostics);
    frame.push(result);
}

fn increment_local(instruction: &InstructionIr, frame: &mut Frame) {
    let InstructionOperandIr::Increment { local, .. } = instruction.operand else {
        return;
    };
    frame.set_local(local, InferredType::Int);
}

fn allocate_object(instruction: &InstructionIr, frame: &mut Frame) {
    let class_name = type_name(instruction).and_then(|name| ClassName::parse(name).ok());
    match class_name {
        Some(class_name) => frame.push_allocation(class_name, instruction.offset),
        None => frame.push(InferredType::Reference(ReferenceType::Unknown)),
    }
}

fn cast_reference(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut value = pop_value(frame, method, instruction, diagnostics);
    let reference = type_name(instruction)
        .and_then(reference_descriptor)
        .map(|descriptor| match descriptor {
            TypeDescriptor::Reference(class_name) => ReferenceType::Exact(class_name),
            descriptor @ TypeDescriptor::Array { .. } => ReferenceType::Array(descriptor),
            TypeDescriptor::Primitive(_) => ReferenceType::Unknown,
        })
        .unwrap_or(ReferenceType::Unknown);
    if !matches!(value.value, InferredType::Reference(ReferenceType::Null)) {
        value.value = InferredType::Reference(reference);
    }
    frame.push_value(value);
}

fn instance_of(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let value = pop_value(frame, method, instruction, diagnostics);
    let reference = type_name(instruction)
        .and_then(reference_descriptor)
        .and_then(|descriptor| match descriptor {
            TypeDescriptor::Reference(class_name) => Some(ReferenceType::Exact(class_name)),
            descriptor @ TypeDescriptor::Array { .. } => Some(ReferenceType::Array(descriptor)),
            TypeDescriptor::Primitive(_) => None,
        });
    let fact = value
        .local_origin
        .zip(reference)
        .map(|(origin, reference)| InstanceOfFact { origin, reference });
    frame.push_instanceof_result(fact);
}

pub(super) fn type_name(instruction: &InstructionIr) -> Option<&str> {
    match &instruction.operand {
        InstructionOperandIr::Type { type_name, .. }
        | InstructionOperandIr::MultiArray { type_name, .. } => type_name.as_deref(),
        _ => None,
    }
}

fn reference_descriptor(name: &str) -> Option<TypeDescriptor> {
    if name.starts_with('[') {
        TypeDescriptor::parse(name).ok()
    } else {
        ClassName::parse(name).ok().map(TypeDescriptor::Reference)
    }
}

fn push_constant(instruction: &InstructionIr, frame: &mut Frame) {
    let value = match &instruction.operand {
        InstructionOperandIr::Constant(ConstantKind::Integer) => {
            InferredType::Integral(IntegralTypeSet::ALL)
        }
        InstructionOperandIr::Constant(ConstantKind::Float) => InferredType::Float,
        InstructionOperandIr::Constant(ConstantKind::Long) => InferredType::Long,
        InstructionOperandIr::Constant(ConstantKind::Double) => InferredType::Double,
        InstructionOperandIr::Constant(ConstantKind::String) => {
            InferredType::Reference(ReferenceType::Exact(ClassName::java_lang_string()))
        }
        InstructionOperandIr::Constant(ConstantKind::Type) => {
            InferredType::Reference(ReferenceType::Exact(ClassName::java_lang_class()))
        }
        InstructionOperandIr::Constant(ConstantKind::MethodHandle) => InferredType::Reference(
            ReferenceType::Exact(ClassName::java_lang_invoke_method_handle()),
        ),
        InstructionOperandIr::Constant(ConstantKind::MethodType) => InferredType::Reference(
            ReferenceType::Exact(ClassName::java_lang_invoke_method_type()),
        ),
        InstructionOperandIr::Constant(ConstantKind::Dynamic(descriptor)) => {
            inferred_from_descriptor(descriptor)
        }
        InstructionOperandIr::Constant(ConstantKind::Unresolved) => {
            InferredType::Reference(ReferenceType::Unknown)
        }
        _ => InferredType::Reference(ReferenceType::Unknown),
    };
    frame.push(value);
}

fn push_subroutine_return_address(
    method: &MethodIr,
    instruction: &InstructionIr,
    frame: &mut Frame,
) {
    let return_target = method
        .instructions
        .iter()
        .skip_while(|candidate| candidate.offset != instruction.offset)
        .nth(1)
        .map(|candidate| candidate.offset);

    match return_target {
        Some(return_target) => frame.push_return_address(return_target),
        None => frame.push(InferredType::ReturnAddress),
    }
}

use std::collections::BTreeSet;

use crate::{ClassName, InferredType, MethodDescriptor, ReferenceType, TypeDescriptor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) locals: Vec<InferredType>,
    pub(crate) stack: Vec<InferredType>,
    local_return_targets: Vec<Option<BTreeSet<u16>>>,
    stack_return_targets: Vec<Option<BTreeSet<u16>>>,
    stack_local_origins: Vec<Option<u16>>,
    stack_instanceof_facts: Vec<Option<InstanceOfFact>>,
}

#[derive(Debug, Clone)]
pub(crate) struct FrameValue {
    pub(crate) value: InferredType,
    return_targets: Option<BTreeSet<u16>>,
    pub(crate) local_origin: Option<u16>,
    instanceof_fact: Option<InstanceOfFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstanceOfFact {
    pub(crate) local: u16,
    pub(crate) reference: ReferenceType,
}

impl FrameValue {
    pub(crate) fn plain(value: InferredType) -> Self {
        Self {
            value,
            return_targets: None,
            local_origin: None,
            instanceof_fact: None,
        }
    }
}

pub(crate) struct MergeOutcome {
    pub(crate) stack_height_mismatch: bool,
}

impl Frame {
    pub(crate) fn entry(
        owner: &ClassName,
        descriptor: &MethodDescriptor,
        access_flags: u16,
        max_locals: u16,
    ) -> Self {
        let mut locals = vec![InferredType::Bottom; usize::from(max_locals)];
        let mut slot = 0_usize;

        if access_flags & 0x0008 == 0 {
            locals.resize(slot + 1, InferredType::Bottom);
            locals[slot] = InferredType::Reference(ReferenceType::Exact(owner.clone()));
            slot += 1;
        }

        for parameter in descriptor.parameters() {
            let value = inferred_from_descriptor(parameter);
            locals.resize(
                slot + usize::from(parameter.slot_width()),
                InferredType::Bottom,
            );
            locals[slot] = value;
            slot += usize::from(parameter.slot_width());
        }

        Self {
            local_return_targets: vec![None; locals.len()],
            locals,
            stack: Vec::new(),
            stack_return_targets: Vec::new(),
            stack_local_origins: Vec::new(),
            stack_instanceof_facts: Vec::new(),
        }
    }

    pub(crate) fn set_local(&mut self, local: u16, value: InferredType) {
        self.set_local_value(local, FrameValue::plain(value));
    }

    pub(crate) fn get_local_value(&self, local: u16) -> FrameValue {
        let local = usize::from(local);
        FrameValue {
            value: self
                .locals
                .get(local)
                .cloned()
                .unwrap_or(InferredType::Bottom),
            return_targets: self
                .local_return_targets
                .get(local)
                .cloned()
                .unwrap_or(None),
            local_origin: None,
            instanceof_fact: None,
        }
    }

    pub(crate) fn set_local_value(&mut self, local: u16, value: FrameValue) {
        let local = usize::from(local);
        let width = usize::from(matches!(
            &value.value,
            InferredType::Long | InferredType::Double
        )) + 1;
        let length = self.locals.len().max(local + width);
        self.locals.resize(length, InferredType::Bottom);
        self.local_return_targets.resize(length, None);
        self.locals[local] = value.value;
        self.local_return_targets[local] = value.return_targets;
        if width == 2 {
            self.locals[local + 1] = InferredType::Bottom;
            self.local_return_targets[local + 1] = None;
        }
    }

    pub(crate) fn push(&mut self, value: InferredType) {
        self.push_value(FrameValue::plain(value));
    }

    pub(crate) fn pop_value(&mut self) -> Option<FrameValue> {
        let value = self.stack.pop()?;
        let return_targets = self.stack_return_targets.pop().unwrap_or(None);
        let local_origin = self.stack_local_origins.pop().unwrap_or(None);
        let instanceof_fact = self.stack_instanceof_facts.pop().unwrap_or(None);
        Some(FrameValue {
            value,
            return_targets,
            local_origin,
            instanceof_fact,
        })
    }

    pub(crate) fn push_value(&mut self, value: FrameValue) {
        self.stack.push(value.value);
        self.stack_return_targets.push(value.return_targets);
        self.stack_local_origins.push(value.local_origin);
        self.stack_instanceof_facts.push(value.instanceof_fact);
    }

    pub(crate) fn push_return_address(&mut self, target: u16) {
        self.stack.push(InferredType::ReturnAddress);
        self.stack_return_targets
            .push(Some(BTreeSet::from([target])));
        self.stack_local_origins.push(None);
        self.stack_instanceof_facts.push(None);
    }

    pub(crate) fn clear_stack(&mut self) {
        self.stack.clear();
        self.stack_return_targets.clear();
        self.stack_local_origins.clear();
        self.stack_instanceof_facts.clear();
    }

    pub(crate) fn local_return_targets(&self, local: u16) -> Option<&BTreeSet<u16>> {
        self.local_return_targets.get(usize::from(local))?.as_ref()
    }

    pub(crate) fn push_local(&mut self, local: u16) {
        let mut value = self.get_local_value(local);
        value.local_origin = Some(local);
        self.push_value(value);
    }

    pub(crate) fn push_instanceof_result(&mut self, fact: Option<InstanceOfFact>) {
        let mut value = FrameValue::plain(InferredType::Int);
        value.instanceof_fact = fact;
        self.push_value(value);
    }

    pub(crate) fn top_instanceof_fact(&self) -> Option<InstanceOfFact> {
        self.stack_instanceof_facts.last().cloned().flatten()
    }

    pub(crate) fn refine_local(&mut self, local: u16, reference: ReferenceType) {
        let local = usize::from(local);
        if let Some(value) = self.locals.get_mut(local)
            && matches!(value, InferredType::Reference(_))
        {
            *value = InferredType::Reference(reference);
            self.local_return_targets[local] = None;
        }
    }

    pub(crate) fn replace_uninitialized(&mut self, allocation_offset: u16, class_name: ClassName) {
        let initialized = InferredType::Reference(ReferenceType::Exact(class_name));
        for value in self.locals.iter_mut().chain(self.stack.iter_mut()) {
            if matches!(
                value,
                InferredType::Uninitialized {
                    allocation_offset: candidate,
                    ..
                } if *candidate == allocation_offset
            ) {
                *value = initialized.clone();
            }
        }
    }

    pub(crate) fn exception_frame(&self, catch_type: Option<ClassName>) -> Self {
        Self {
            locals: self.locals.clone(),
            stack: vec![InferredType::Reference(match catch_type {
                Some(class_name) => ReferenceType::Exact(class_name),
                None => ReferenceType::Exact(ClassName::java_lang_throwable()),
            })],
            local_return_targets: self.local_return_targets.clone(),
            stack_return_targets: vec![None],
            stack_local_origins: vec![None],
            stack_instanceof_facts: vec![None],
        }
    }

    pub(crate) fn merge_from(&mut self, incoming: &Self) -> MergeOutcome {
        let mut stack_height_mismatch = false;

        let local_count = self.locals.len().max(incoming.locals.len());
        self.locals.resize(local_count, InferredType::Bottom);
        self.local_return_targets.resize(local_count, None);
        for (index, value) in incoming.locals.iter().enumerate() {
            let existing = self.locals[index].clone();
            let merged = self.locals[index].join(value);
            self.local_return_targets[index] = merged_return_targets(
                &existing,
                self.local_return_targets[index].as_ref(),
                value,
                incoming
                    .local_return_targets
                    .get(index)
                    .and_then(Option::as_ref),
                &merged,
            );
            if merged != self.locals[index] {
                self.locals[index] = merged;
            }
        }

        if self.stack.len() != incoming.stack.len() {
            let stack_len = self.stack.len().max(incoming.stack.len());
            let merged = vec![InferredType::Conflict; stack_len];
            if self.stack != merged {
                self.stack = merged;
                self.stack_return_targets = vec![None; stack_len];
                self.stack_local_origins = vec![None; stack_len];
                self.stack_instanceof_facts = vec![None; stack_len];
            }
            stack_height_mismatch = true;
        } else {
            for index in 0..self.stack.len() {
                let existing = self.stack[index].clone();
                let value = &incoming.stack[index];
                let merged = existing.join(value);
                self.stack_return_targets[index] = merged_return_targets(
                    &existing,
                    self.stack_return_targets[index].as_ref(),
                    value,
                    incoming
                        .stack_return_targets
                        .get(index)
                        .and_then(Option::as_ref),
                    &merged,
                );
                if merged != self.stack[index] {
                    self.stack[index] = merged;
                }
                if self.stack_local_origins[index] != incoming.stack_local_origins[index] {
                    self.stack_local_origins[index] = None;
                }
                if self.stack_instanceof_facts[index] != incoming.stack_instanceof_facts[index] {
                    self.stack_instanceof_facts[index] = None;
                }
            }
        }

        MergeOutcome {
            stack_height_mismatch,
        }
    }
}

fn merged_return_targets(
    existing: &InferredType,
    existing_targets: Option<&BTreeSet<u16>>,
    incoming: &InferredType,
    incoming_targets: Option<&BTreeSet<u16>>,
    merged: &InferredType,
) -> Option<BTreeSet<u16>> {
    if !matches!(merged, InferredType::ReturnAddress) {
        return None;
    }

    let mut targets = BTreeSet::new();
    let mut known = true;
    for (value, candidate) in [(existing, existing_targets), (incoming, incoming_targets)] {
        if matches!(value, InferredType::ReturnAddress) {
            let Some(candidate) = candidate else {
                known = false;
                continue;
            };
            targets.extend(candidate.iter().copied());
        }
    }

    known.then_some(targets)
}

pub(crate) fn inferred_from_descriptor(descriptor: &TypeDescriptor) -> InferredType {
    match descriptor {
        TypeDescriptor::Primitive(primitive) => match primitive {
            crate::PrimitiveType::Long => InferredType::Long,
            crate::PrimitiveType::Float => InferredType::Float,
            crate::PrimitiveType::Double => InferredType::Double,
            crate::PrimitiveType::Boolean
            | crate::PrimitiveType::Byte
            | crate::PrimitiveType::Char
            | crate::PrimitiveType::Short
            | crate::PrimitiveType::Int => InferredType::Int,
        },
        TypeDescriptor::Reference(class_name) => {
            InferredType::Reference(ReferenceType::Exact(class_name.clone()))
        }
        TypeDescriptor::Array { .. } => {
            InferredType::Reference(ReferenceType::Array(descriptor.clone()))
        }
    }
}

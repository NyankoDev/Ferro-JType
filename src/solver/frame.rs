use crate::{ClassName, InferredType, MethodDescriptor, ReferenceType, TypeDescriptor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) locals: Vec<InferredType>,
    pub(crate) stack: Vec<InferredType>,
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
            locals,
            stack: Vec::new(),
        }
    }

    #[must_use]
    pub(crate) fn get_local(&self, local: u16) -> InferredType {
        self.locals
            .get(usize::from(local))
            .cloned()
            .unwrap_or(InferredType::Bottom)
    }

    pub(crate) fn set_local(&mut self, local: u16, value: InferredType) {
        let local = usize::from(local);
        self.locals.resize(local + 1, InferredType::Bottom);
        self.locals[local] = value;
    }

    pub(crate) fn pop(&mut self) -> Option<InferredType> {
        self.stack.pop()
    }

    pub(crate) fn push(&mut self, value: InferredType) {
        self.stack.push(value);
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
        }
    }

    pub(crate) fn merge_from(&mut self, incoming: &Self) -> MergeOutcome {
        let mut stack_height_mismatch = false;

        let local_count = self.locals.len().max(incoming.locals.len());
        self.locals.resize(local_count, InferredType::Bottom);
        for (index, value) in incoming.locals.iter().enumerate() {
            let merged = self.locals[index].join(value);
            if merged != self.locals[index] {
                self.locals[index] = merged;
            }
        }

        if self.stack.len() != incoming.stack.len() {
            let stack_len = self.stack.len().max(incoming.stack.len());
            let merged = vec![InferredType::Conflict; stack_len];
            if self.stack != merged {
                self.stack = merged;
            }
            stack_height_mismatch = true;
        } else {
            for (existing, value) in self.stack.iter_mut().zip(&incoming.stack) {
                let merged = existing.join(value);
                if merged != *existing {
                    *existing = merged;
                }
            }
        }

        MergeOutcome {
            stack_height_mismatch,
        }
    }
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

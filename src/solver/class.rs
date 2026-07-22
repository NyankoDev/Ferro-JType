use std::collections::{HashMap, VecDeque};

use crate::ir::{ClassIr, InstructionIr, InstructionOperandIr, MemberRefIr, MethodIr};
use crate::{
    ClassInference, ClassName, Diagnostic, DiagnosticKind, DiagnosticLocation, DiagnosticSeverity,
    Error, FieldSummaries, InferenceConfig, InferredType, MethodDescriptor, MethodInference,
    MethodInvocationKind, MethodSummaries, MethodSummaryResolver,
};

use super::engine::analyze_method;
use super::fields::{
    StaticFieldResolver, local_field_readers, update_local_static_field_summaries,
};

pub(crate) fn analyze_class(
    class: &ClassIr,
    config: &InferenceConfig,
) -> Result<ClassInference, Error> {
    let method_indices = local_method_indices(class);
    let local_calls = LocalMethodCalls {
        owner: &class.name,
        class_is_final: class.access_flags & 0x0010 != 0,
        methods: &class.methods,
        method_indices: &method_indices,
    };
    let callers = local_summary_callers(class, &local_calls);
    let field_readers = local_field_readers(class);
    let mut summaries = MethodSummaries::new();
    let mut returned_parameters = HashMap::new();
    let mut field_summaries = FieldSummaries::new();
    let mut analyses = (0..class.methods.len())
        .map(|_| None)
        .collect::<Vec<Option<(MethodInference, Vec<Diagnostic>)>>>();
    let mut scheduled = vec![true; class.methods.len()];
    let mut worklist = VecDeque::from_iter(0..class.methods.len());
    let mut reanalysis_items = 0_usize;
    let mut summary_analysis_complete = true;

    while let Some(method_index) = worklist.pop_front() {
        scheduled[method_index] = false;
        if analyses[method_index].is_some() {
            reanalysis_items += 1;
            if !config.unbounded_analysis() && reanalysis_items > config.max_work_items() {
                summary_analysis_complete = false;
                break;
            }
        }

        let (inference, method_diagnostics) = {
            let method_resolver = ClassSummaryResolver {
                external: config.method_summaries(),
                local: &summaries,
                local_calls: &local_calls,
                returned_parameters: &returned_parameters,
            };
            let field_resolver =
                StaticFieldResolver::new(config.field_summaries(), &field_summaries);
            analyze_method(
                &class.name,
                &class.methods[method_index],
                config,
                Some(&method_resolver),
                Some(&field_resolver),
            )
        };
        let summary_changed = update_local_method_summary(&mut summaries, &class.name, &inference);
        let parameter_return_changed =
            update_local_parameter_return(&mut returned_parameters, &inference);
        let changed_fields = update_local_static_field_summaries(
            class,
            &class.methods[method_index],
            &inference,
            &mut field_summaries,
            config.type_hierarchy(),
        );
        analyses[method_index] = Some((inference, method_diagnostics));

        if summary_changed || parameter_return_changed {
            for caller in &callers[method_index] {
                if !scheduled[*caller] {
                    scheduled[*caller] = true;
                    worklist.push_back(*caller);
                }
            }
        }
        for field in changed_fields {
            let Some(readers) = field_readers.get(&field) else {
                continue;
            };
            for reader in readers {
                if !scheduled[*reader] {
                    scheduled[*reader] = true;
                    worklist.push_back(*reader);
                }
            }
        }
    }

    let mut diagnostics = class.diagnostics.clone();
    if !summary_analysis_complete {
        diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Error,
            DiagnosticKind::AnalysisLimitReached,
            DiagnosticLocation::class_level(),
            "class-local method-summary work-item budget was reached",
        ));
    }
    let methods = analyses
        .into_iter()
        .map(|analysis| {
            let (mut inference, method_diagnostics) =
                analysis.expect("every method is analyzed before class-local summary convergence");
            if !summary_analysis_complete {
                inference.mark_analysis_incomplete();
            }
            diagnostics.extend(method_diagnostics);
            inference
        })
        .collect();

    if config.strict()
        && let Some(diagnostic) = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity() != DiagnosticSeverity::Note)
    {
        return Err(Error::StrictAnalysis {
            message: diagnostic.message().to_owned(),
        });
    }

    Ok(ClassInference::new(
        class.name.clone(),
        class.generic_signature.clone(),
        methods,
        diagnostics,
    ))
}

struct ClassSummaryResolver<'a> {
    external: Option<&'a dyn MethodSummaryResolver>,
    local: &'a MethodSummaries,
    local_calls: &'a LocalMethodCalls<'a>,
    returned_parameters: &'a HashMap<MethodKey, usize>,
}

impl MethodSummaryResolver for ClassSummaryResolver<'_> {
    fn return_type(
        &self,
        owner: &ClassName,
        name: &str,
        descriptor: &MethodDescriptor,
    ) -> Option<InferredType> {
        self.external
            .and_then(|resolver| resolver.return_type(owner, name, descriptor))
    }

    fn return_type_for_invocation(
        &self,
        owner: &ClassName,
        name: &str,
        descriptor: &MethodDescriptor,
        invocation_kind: MethodInvocationKind,
    ) -> Option<InferredType> {
        self.external
            .and_then(|resolver| {
                resolver.return_type_for_invocation(owner, name, descriptor, invocation_kind)
            })
            .or_else(|| {
                local_call_is_deterministic(
                    self.local_calls,
                    owner,
                    name,
                    descriptor,
                    invocation_kind,
                    false,
                )
                .then(|| self.local.return_type(owner, name, descriptor))
                .flatten()
            })
    }

    fn return_type_for_call(
        &self,
        owner: &ClassName,
        name: &str,
        descriptor: &MethodDescriptor,
        invocation_kind: MethodInvocationKind,
        receiver_is_exact_allocation: bool,
    ) -> Option<InferredType> {
        self.external
            .and_then(|resolver| {
                resolver.return_type_for_call(
                    owner,
                    name,
                    descriptor,
                    invocation_kind,
                    receiver_is_exact_allocation,
                )
            })
            .or_else(|| {
                local_call_is_deterministic(
                    self.local_calls,
                    owner,
                    name,
                    descriptor,
                    invocation_kind,
                    receiver_is_exact_allocation,
                )
                .then(|| self.local.return_type(owner, name, descriptor))
                .flatten()
            })
    }

    fn returned_parameter_index_for_invocation(
        &self,
        owner: &ClassName,
        name: &str,
        descriptor: &MethodDescriptor,
        invocation_kind: MethodInvocationKind,
    ) -> Option<usize> {
        if self
            .external
            .and_then(|resolver| {
                resolver.return_type_for_invocation(owner, name, descriptor, invocation_kind)
            })
            .is_some()
        {
            return None;
        }
        local_call_is_deterministic(
            self.local_calls,
            owner,
            name,
            descriptor,
            invocation_kind,
            false,
        )
        .then(|| {
            self.returned_parameters
                .get(&MethodKey {
                    name: name.to_owned(),
                    descriptor: descriptor.clone(),
                })
                .copied()
        })
        .flatten()
    }

    fn returned_parameter_index_for_call(
        &self,
        owner: &ClassName,
        name: &str,
        descriptor: &MethodDescriptor,
        invocation_kind: MethodInvocationKind,
        receiver_is_exact_allocation: bool,
    ) -> Option<usize> {
        if self
            .external
            .and_then(|resolver| {
                resolver.return_type_for_call(
                    owner,
                    name,
                    descriptor,
                    invocation_kind,
                    receiver_is_exact_allocation,
                )
            })
            .is_some()
        {
            return None;
        }
        local_call_is_deterministic(
            self.local_calls,
            owner,
            name,
            descriptor,
            invocation_kind,
            receiver_is_exact_allocation,
        )
        .then(|| {
            self.returned_parameters
                .get(&MethodKey {
                    name: name.to_owned(),
                    descriptor: descriptor.clone(),
                })
                .copied()
        })
        .flatten()
    }
}

fn update_local_method_summary(
    summaries: &mut MethodSummaries,
    owner: &ClassName,
    method: &MethodInference,
) -> bool {
    let previous = summaries.return_type(owner, method.name(), method.descriptor());
    let next = method.inferred_return_type().cloned();
    if previous == next {
        return false;
    }

    match next {
        Some(return_type) => {
            summaries.insert_return_type(
                owner.clone(),
                method.name(),
                method.descriptor().clone(),
                return_type,
            );
        }
        None => {
            summaries.remove_return_type(owner, method.name(), method.descriptor());
        }
    }
    true
}

fn update_local_parameter_return(
    returned_parameters: &mut HashMap<MethodKey, usize>,
    method: &MethodInference,
) -> bool {
    let key = MethodKey {
        name: method.name().to_owned(),
        descriptor: method.descriptor().clone(),
    };
    let next = method.returned_parameter_index();
    if returned_parameters.get(&key).copied() == next {
        return false;
    }
    match next {
        Some(index) => {
            returned_parameters.insert(key, index);
        }
        None => {
            returned_parameters.remove(&key);
        }
    }
    true
}

fn local_method_indices(class: &ClassIr) -> HashMap<MethodKey, usize> {
    class
        .methods
        .iter()
        .enumerate()
        .map(|(index, method)| (MethodKey::from_method(method), index))
        .collect()
}

fn local_summary_callers(class: &ClassIr, local_calls: &LocalMethodCalls<'_>) -> Vec<Vec<usize>> {
    let mut callers = vec![Vec::new(); class.methods.len()];

    for (caller_index, method) in class.methods.iter().enumerate() {
        for instruction in method
            .instructions
            .iter()
            .filter(|instruction| matches!(instruction.opcode, 0xb6..=0xb8))
        {
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

fn local_call_is_deterministic(
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

struct LocalMethodCalls<'a> {
    owner: &'a ClassName,
    class_is_final: bool,
    methods: &'a [MethodIr],
    method_indices: &'a HashMap<MethodKey, usize>,
}

fn resolved_method_reference(instruction: &InstructionIr) -> Option<(&ClassName, &str, &str)> {
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MethodKey {
    name: String,
    descriptor: MethodDescriptor,
}

impl MethodKey {
    fn from_method(method: &MethodIr) -> Self {
        Self {
            name: method.name.clone(),
            descriptor: method.descriptor.clone(),
        }
    }
}

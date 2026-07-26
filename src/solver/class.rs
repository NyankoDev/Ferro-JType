use std::collections::{HashMap, HashSet, VecDeque};

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
    analyze_class_with_method_summaries(class, config, config.method_summaries())
}

fn analyze_class_with_method_summaries(
    class: &ClassIr,
    config: &InferenceConfig,
    external_method_summaries: Option<&dyn MethodSummaryResolver>,
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
                external: external_method_summaries,
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

pub(crate) fn analyze_classes(
    classes: &[ClassIr],
    config: &InferenceConfig,
) -> Result<Vec<ClassInference>, Error> {
    let callers = batch_summary_callers(classes);
    let targets = BatchCallTargets::from_classes(classes);
    let mut summaries = MethodSummaries::new();
    let mut analyses = (0..classes.len())
        .map(|_| None)
        .collect::<Vec<Option<ClassInference>>>();
    let mut scheduled = vec![true; classes.len()];
    let mut worklist = VecDeque::from_iter(0..classes.len());
    let mut reanalysis_items = 0_usize;
    let mut analysis_complete = true;

    while let Some(class_index) = worklist.pop_front() {
        scheduled[class_index] = false;
        if analyses[class_index].is_some() {
            reanalysis_items += 1;
            if !config.unbounded_analysis() && reanalysis_items > config.max_work_items() {
                analysis_complete = false;
                break;
            }
        }

        let class = &classes[class_index];
        let resolver = BatchSummaryResolver {
            external: config.method_summaries(),
            summaries: &summaries,
            targets: &targets,
            current_owner: &class.name,
        };
        let inference = analyze_class_with_method_summaries(class, config, Some(&resolver))?;
        let changed = update_batch_method_summaries(&mut summaries, &inference);
        analyses[class_index] = Some(inference);

        for key in changed {
            let Some(callers) = callers.get(&key) else {
                continue;
            };
            for caller in callers {
                if !scheduled[*caller] {
                    scheduled[*caller] = true;
                    worklist.push_back(*caller);
                }
            }
        }
    }

    let mut analyses = analyses
        .into_iter()
        .map(|analysis| analysis.expect("every batch class is analyzed before summary convergence"))
        .collect::<Vec<_>>();
    if !analysis_complete {
        for analysis in &mut analyses {
            analysis.mark_batch_analysis_incomplete();
        }
    }

    if config.strict()
        && let Some(diagnostic) = analyses
            .iter()
            .flat_map(|analysis| analysis.diagnostics())
            .find(|diagnostic| diagnostic.severity() != DiagnosticSeverity::Note)
    {
        return Err(Error::StrictAnalysis {
            message: diagnostic.message().to_owned(),
        });
    }

    Ok(analyses)
}

struct BatchSummaryResolver<'a> {
    external: Option<&'a dyn MethodSummaryResolver>,
    summaries: &'a MethodSummaries,
    targets: &'a BatchCallTargets,
    current_owner: &'a ClassName,
}

impl BatchSummaryResolver<'_> {
    fn batch_return_type(
        &self,
        owner: &ClassName,
        name: &str,
        descriptor: &MethodDescriptor,
        invocation_kind: MethodInvocationKind,
        receiver_is_exact_allocation: bool,
    ) -> Option<InferredType> {
        if owner == self.current_owner
            || !self.targets.is_deterministic(
                owner,
                name,
                descriptor,
                invocation_kind,
                receiver_is_exact_allocation,
            )
        {
            return None;
        }
        self.summaries.return_type(owner, name, descriptor)
    }
}

impl MethodSummaryResolver for BatchSummaryResolver<'_> {
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
            .or_else(|| self.batch_return_type(owner, name, descriptor, invocation_kind, false))
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
                self.batch_return_type(
                    owner,
                    name,
                    descriptor,
                    invocation_kind,
                    receiver_is_exact_allocation,
                )
            })
    }
}

fn update_batch_method_summaries(
    summaries: &mut MethodSummaries,
    inference: &ClassInference,
) -> Vec<BatchMethodKey> {
    let mut changed = Vec::new();
    for method in inference.methods() {
        let key = BatchMethodKey {
            owner: inference.class_name().clone(),
            method: MethodKey {
                name: method.name().to_owned(),
                descriptor: method.descriptor().clone(),
            },
        };
        let previous = summaries.return_type(&key.owner, &key.method.name, &key.method.descriptor);
        let next = method.inferred_return_type().cloned();
        if previous == next {
            continue;
        }
        match next {
            Some(return_type) => {
                summaries.insert_return_type(
                    key.owner.clone(),
                    key.method.name.clone(),
                    key.method.descriptor.clone(),
                    return_type,
                );
            }
            None => {
                summaries.remove_return_type(&key.owner, &key.method.name, &key.method.descriptor);
            }
        }
        changed.push(key);
    }
    changed
}

fn batch_summary_callers(classes: &[ClassIr]) -> HashMap<BatchMethodKey, Vec<usize>> {
    let mut callers = HashMap::<BatchMethodKey, Vec<usize>>::new();
    for (class_index, class) in classes.iter().enumerate() {
        for method in &class.methods {
            for instruction in method
                .instructions
                .iter()
                .filter(|instruction| matches!(instruction.opcode, 0xb6..=0xb8))
            {
                let Some((owner, name, descriptor)) = resolved_method_reference(instruction) else {
                    continue;
                };
                let Ok(descriptor) = MethodDescriptor::parse(descriptor) else {
                    continue;
                };
                callers
                    .entry(BatchMethodKey {
                        owner: owner.clone(),
                        method: MethodKey {
                            name: name.to_owned(),
                            descriptor,
                        },
                    })
                    .or_default()
                    .push(class_index);
            }
        }
    }
    for callers in callers.values_mut() {
        callers.sort_unstable();
        callers.dedup();
    }
    callers
}

struct BatchCallTargets {
    final_classes: HashSet<ClassName>,
    final_methods: HashSet<BatchMethodKey>,
}

impl BatchCallTargets {
    fn from_classes(classes: &[ClassIr]) -> Self {
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

    fn is_deterministic(
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BatchMethodKey {
    owner: ClassName,
    method: MethodKey,
}

impl MethodKey {
    fn from_method(method: &MethodIr) -> Self {
        Self {
            name: method.name.clone(),
            descriptor: method.descriptor.clone(),
        }
    }
}

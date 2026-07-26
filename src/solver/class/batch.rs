use std::collections::{HashMap, VecDeque};

use crate::ir::ClassIr;
use crate::{
    ClassInference, ClassName, DiagnosticSeverity, Error, InferenceConfig, InferredType,
    MethodDescriptor, MethodInvocationKind, MethodSummaries, MethodSummaryResolver,
};

use super::resolution::{BatchCallTargets, BatchMethodKey, MethodKey, resolved_method_reference};
use super::summaries::analyze_class_with_method_summaries;

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

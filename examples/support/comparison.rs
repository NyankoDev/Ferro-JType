use std::collections::{BTreeMap, BTreeSet};

use ferro_jtype::{ClassInferences, InferredType, IntegralTypeSet, ReferenceType};

pub fn report(label: &str, result: &ClassInferences) {
    let mut methods = 0;
    let mut instructions = 0;
    let mut diagnostics = 0;
    let mut unknown = 0;
    let mut conflict = 0;
    let mut bottom = 0;
    let mut alternatives = 0;
    let mut integral_all = 0;
    let mut expectations = 0;
    for class in result.classes() {
        diagnostics += class.diagnostics().len();
        for method in class.methods() {
            methods += 1;
            for instruction in method.instructions() {
                instructions += 1;
                expectations += instruction.operand_expectations().len();
                for value in instruction
                    .stack_before()
                    .iter()
                    .chain(instruction.stack_after())
                {
                    unknown += usize::from(contains(
                        value,
                        &InferredType::Reference(ReferenceType::Unknown),
                    ));
                    conflict += usize::from(contains(value, &InferredType::Conflict));
                    bottom += usize::from(contains(value, &InferredType::Bottom));
                    alternatives += usize::from(matches!(value, InferredType::Alternatives(_)));
                    integral_all +=
                        usize::from(value.integral_types() == Some(IntegralTypeSet::ALL));
                }
            }
        }
    }
    println!(
        "{label}: classes={}, methods={methods}, instructions={instructions}, complete={}, diagnostics={diagnostics}, stack unknown={unknown}, conflict={conflict}, bottom={bottom}, alternatives={alternatives}",
        result.len(),
        result.analysis_complete()
    );
    println!(
        "  operand constraints={expectations}, stack values with all integral candidates={integral_all}"
    );
}

fn contains(value: &InferredType, needle: &InferredType) -> bool {
    value == needle
        || matches!(value, InferredType::Alternatives(values) if values.iter().any(|value| contains(value, needle)))
}

pub fn compare(left: &ClassInferences, right: &ClassInferences) {
    let mut changed = 0;
    let mut stacks = 0;
    let mut locals = 0;
    let mut coverage = 0;
    let mut returns = 0;
    let mut method_locals = 0;
    let mut constraints = 0;
    assert_eq!(
        left.len(),
        right.len(),
        "the same inputs must retain class metadata"
    );
    for (class, other) in left.classes().iter().zip(right.classes()) {
        assert_eq!(class.class_name(), other.class_name());
        assert_eq!(class.methods().len(), other.methods().len());
        for (method, other) in class.methods().iter().zip(other.methods()) {
            assert_eq!(method.name(), other.name());
            assert_eq!(method.descriptor(), other.descriptor());
            returns += usize::from(method.inferred_return_type() != other.inferred_return_type());
            method_locals += usize::from(method.local_types() != other.local_types());
            let before = method
                .instructions()
                .iter()
                .map(|i| (i.bytecode_offset(), i))
                .collect::<BTreeMap<_, _>>();
            let after = other
                .instructions()
                .iter()
                .map(|i| (i.bytecode_offset(), i))
                .collect::<BTreeMap<_, _>>();
            let offsets = before
                .keys()
                .chain(after.keys())
                .copied()
                .collect::<BTreeSet<_>>();
            let mut method_changes = 0;
            for offset in offsets {
                let (Some(instruction), Some(other)) = (before.get(&offset), after.get(&offset))
                else {
                    coverage += 1;
                    continue;
                };
                let local_changed = instruction.local_types() != other.local_types();
                let stack_changed = instruction.stack_before() != other.stack_before()
                    || instruction.stack_after() != other.stack_after();
                locals += usize::from(local_changed);
                stacks += usize::from(stack_changed);
                constraints +=
                    usize::from(instruction.operand_expectations() != other.operand_expectations());
                if local_changed || stack_changed {
                    changed += 1;
                    method_changes += 1;
                }
            }
            if method_changes != 0 {
                println!(
                    "  {}.{} {:?}: {method_changes} instruction states differ",
                    class.class_name(),
                    method.name(),
                    method.descriptor()
                );
            }
        }
    }
    println!(
        "Differences: instructions={changed}, stacks={stacks}, locals={locals}, reachability={coverage}, returns={returns}, method-local summaries={method_locals}, operand constraints={constraints}"
    );
    println!(
        "Bottom in unused locals and category-two companion slots is not a missing type. Reference alternatives are retained evidence, not Unknown. Completion is not proof of recovered source types."
    );
}

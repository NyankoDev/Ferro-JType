use std::{env, fs, path::Path, process::ExitCode, sync::Arc};

use ferro_jtype::{ClassHierarchy, InferenceConfig, Inferer};

fn main() -> ExitCode {
    let mut arguments = env::args_os().skip(1);
    let Some(path) = arguments.next() else {
        eprintln!("usage: cargo run --example infer -- <class-file> [--reference <jar>]...");
        return ExitCode::FAILURE;
    };

    let mut hierarchy = ClassHierarchy::new();
    while let Some(option) = arguments.next() {
        if option != "--reference" {
            eprintln!("unknown option: {}", option.to_string_lossy());
            return ExitCode::FAILURE;
        }
        let Some(reference) = arguments.next() else {
            eprintln!("--reference requires a JAR path");
            return ExitCode::FAILURE;
        };
        if let Err(error) = hierarchy.insert_jar_file(&reference) {
            eprintln!("failed to index {}: {error}", reference.to_string_lossy());
            return ExitCode::FAILURE;
        }
    }
    let mut config = InferenceConfig::default();
    if !hierarchy.is_empty() {
        config = config.with_shared_type_hierarchy(Arc::new(hierarchy));
    }
    let inferer = match Inferer::new(config) {
        Ok(inferer) => inferer,
        Err(error) => {
            eprintln!("failed to configure inference: {error}");
            return ExitCode::FAILURE;
        }
    };

    let path = Path::new(&path);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("failed to read {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };

    let inference = match inferer.infer_class(&bytes) {
        Ok(inference) => inference,
        Err(error) => {
            eprintln!("failed to infer {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };

    println!("class: {}", inference.class_name().as_str());
    println!("analysis complete: {}", inference.analysis_complete());

    for method in inference.methods() {
        println!("\nmethod: {}", method.name());
        println!("  descriptor: {:?}", method.descriptor());
        println!("  declared return type: {:?}", method.return_type());
        println!(
            "  inferred return type: {:?}",
            method.inferred_return_type()
        );
        println!(
            "  returned parameter index: {:?}",
            method.returned_parameter_index()
        );
        println!("  analysis complete: {}", method.analysis_complete());
        println!("  local types:");

        for (slot, inferred_type) in method.local_types().iter().enumerate() {
            println!("    {slot}: {inferred_type:?}");
        }

        for instruction in method.instructions() {
            if instruction.operand_expectations().is_empty() {
                continue;
            }
            println!(
                "  operand expectations at {}:",
                instruction.bytecode_offset()
            );
            for expectation in instruction.operand_expectations() {
                println!(
                    "    stack[{}]: {:?}",
                    expectation.stack_index(),
                    expectation.constraint()
                );
            }
        }
    }

    if inference.diagnostics().is_empty() {
        println!("\ndiagnostics: none");
    } else {
        println!("\ndiagnostics:");
        for diagnostic in inference.diagnostics() {
            let location = diagnostic.location();
            println!(
                "  {:?} {:?}: {}",
                diagnostic.severity(),
                diagnostic.kind(),
                diagnostic.message()
            );

            if let Some(method_name) = location.method_name() {
                println!(
                    "    method: {}{}",
                    method_name,
                    location.method_descriptor().unwrap_or_default()
                );
            }
            if let Some(offset) = location.bytecode_offset() {
                println!("    bytecode offset: {offset}");
            }
        }
    }

    ExitCode::SUCCESS
}

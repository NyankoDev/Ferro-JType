use std::{env, error::Error, fs, io::Read, path::Path, process::Command, sync::Arc};

use ferro_jtype::{ClassHierarchy, InferenceConfig, Inferer};

#[path = "support/comparison.rs"]
mod comparison;

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() != 2 {
        return Err("usage: cargo run --example infer_jrt -- <class-or-jar> <jdk-java-executable> (JDK 11+)".into());
    }
    let classes = read_classes(Path::new(&arguments[0]))?;
    let mut input_hierarchy = ClassHierarchy::new();
    for bytes in &classes {
        input_hierarchy.insert_class(bytes)?;
    }
    let without_config =
        InferenceConfig::default().with_shared_type_hierarchy(Arc::new(input_hierarchy));
    let without_jrt = Inferer::new(without_config)?.infer_classes(&classes)?;
    eprintln!("Reading jrt:/ from the selected JDK; target classes are never executed.");
    let export = Command::new(&arguments[1])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/support/ExportJrt.java"
        ))
        .output()?;
    if !export.status.success() {
        return Err(format!(
            "jrt export failed: {}",
            String::from_utf8_lossy(&export.stderr)
        )
        .into());
    }
    let mut hierarchy = ClassHierarchy::new();
    let reference_count = hierarchy.insert_jar(&export.stdout)?;
    drop(export);
    for bytes in &classes {
        hierarchy.insert_class(bytes)?;
    }
    let config = InferenceConfig::default().with_shared_type_hierarchy(Arc::new(hierarchy));
    let with_jrt = Inferer::new(config)?.infer_classes(&classes)?;
    println!("JDK reference classes: {reference_count}");
    println!("Both modes index the input class headers; only the second adds jrt.");
    comparison::report("without jrt", &without_jrt);
    comparison::report("with jrt", &with_jrt);
    comparison::compare(&without_jrt, &with_jrt);
    if !without_jrt.analysis_complete() || !with_jrt.analysis_complete() {
        return Err("analysis did not reach a fixed point".into());
    }
    Ok(())
}

fn read_classes(path: &Path) -> Result<Vec<Vec<u8>>, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    if bytes.starts_with(&[0xca, 0xfe, 0xba, 0xbe]) {
        return Ok(vec![bytes]);
    }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut classes = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if !entry.name().ends_with(".class")
            || entry.name().starts_with("META-INF/")
            || entry.name().rsplit('/').next() == Some("module-info.class")
        {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        classes.push(bytes);
    }
    if classes.is_empty() {
        return Err("input contains no base class files".into());
    }
    Ok(classes)
}

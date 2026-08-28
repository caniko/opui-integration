use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::cert::sha256_file;

#[derive(Debug, Serialize)]
pub struct ArtifactRef {
    path: String,
    sha256: String,
}

#[derive(Debug, Serialize)]
pub struct ComputedChange {
    pub source_id: String,
    pub runtime_id: Option<String>,
    pub field_path: String,
    pub expected: Value,
    pub actual: Value,
    pub numeric_delta: Option<f64>,
    pub parent: Value,
    pub child_order: Value,
    pub computed_rectangle: Value,
    pub content_rectangle: Value,
    pub transform: Value,
    pub target_camera: Value,
    pub stack_index: Value,
    pub clipping_rectangle: Value,
    pub visible: Value,
    pub resolved_border: Value,
    pub resolved_radius: Value,
}

#[derive(Debug, Serialize)]
pub struct ComputedDiffReport {
    actual: ArtifactRef,
    golden: ArtifactRef,
    mapping: ArtifactRef,
    mapping_golden: ArtifactRef,
    case_manifest: ArtifactRef,
    source_manifest: ArtifactRef,
    context: ArtifactRef,
    mapping_equal: bool,
    changes: Vec<ComputedChange>,
}

pub struct ComputedDiffInputs<'a> {
    pub actual: &'a Path,
    pub golden: &'a Path,
    pub mapping: &'a Path,
    pub mapping_golden: &'a Path,
    pub case_manifest: &'a Path,
    pub source_manifest: &'a Path,
    pub context: &'a Path,
    pub output_dir: &'a Path,
}

pub fn write_report(inputs: ComputedDiffInputs<'_>) -> Result<usize, String> {
    let actual_value = read_json(inputs.actual)?;
    let golden_value = read_json(inputs.golden)?;
    let mapping_value = read_json(inputs.mapping)?;
    let mapping_golden_value = read_json(inputs.mapping_golden)?;
    let context_value = read_json(inputs.context)?;
    let runtime_ids = rows_by_source(&mapping_value)?
        .into_iter()
        .filter_map(|(source, row)| {
            row["runtime_id"]
                .as_str()
                .map(|runtime| (source, runtime.to_string()))
        })
        .collect::<BTreeMap<_, _>>();
    let contexts = rows_by_source(&context_value)?;
    let mut changes = Vec::new();
    let actual_rows = rows_by_source(&actual_value)?;
    let golden_rows = rows_by_source(&golden_value)?;
    for source_id in actual_rows
        .keys()
        .chain(golden_rows.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
    {
        let expected = golden_rows.get(&source_id).unwrap_or(&Value::Null);
        let actual = actual_rows.get(&source_id).unwrap_or(&Value::Null);
        let context = contexts.get(&source_id).unwrap_or(&Value::Null);
        let mut fields = Vec::new();
        diff_values("", expected, actual, &mut fields);
        for (field_path, expected, actual) in fields {
            let numeric_delta = actual
                .as_f64()
                .zip(expected.as_f64())
                .map(|(actual, expected)| actual - expected);
            changes.push(ComputedChange {
                source_id: source_id.clone(),
                runtime_id: runtime_ids.get(&source_id).cloned(),
                field_path,
                expected,
                actual,
                numeric_delta,
                parent: context["parent"].clone(),
                child_order: context["child_order"].clone(),
                computed_rectangle: context["computed_rectangle"].clone(),
                content_rectangle: context["content_rectangle"].clone(),
                transform: context["transform"].clone(),
                target_camera: context["target_camera"].clone(),
                stack_index: context["stack_index"].clone(),
                clipping_rectangle: context["clipping_rectangle"].clone(),
                visible: context["visible"].clone(),
                resolved_border: context["resolved_border"].clone(),
                resolved_radius: context["resolved_radius"].clone(),
            });
        }
    }
    let report = ComputedDiffReport {
        actual: artifact(inputs.actual)?,
        golden: artifact(inputs.golden)?,
        mapping: artifact(inputs.mapping)?,
        mapping_golden: artifact(inputs.mapping_golden)?,
        case_manifest: artifact(inputs.case_manifest)?,
        source_manifest: artifact(inputs.source_manifest)?,
        context: artifact(inputs.context)?,
        mapping_equal: mapping_value == mapping_golden_value,
        changes,
    };
    fs::write(
        inputs.output_dir.join("computed-diff.json"),
        serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        inputs.output_dir.join("computed-diff.md"),
        markdown(&report),
    )
    .map_err(|error| error.to_string())?;
    Ok(report.changes.len())
}

fn artifact(path: &Path) -> Result<ArtifactRef, String> {
    Ok(ArtifactRef {
        path: path.display().to_string(),
        sha256: sha256_file(path)?,
    })
}

fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

fn rows_by_source(value: &Value) -> Result<BTreeMap<String, Value>, String> {
    value
        .as_array()
        .ok_or("computed or mapping snapshot is not an array")?
        .iter()
        .map(|row| {
            let source = row["source_id"]
                .as_str()
                .ok_or("snapshot row has no source_id")?;
            Ok((source.to_string(), row.clone()))
        })
        .collect()
}

fn diff_values(
    path: &str,
    expected: &Value,
    actual: &Value,
    changes: &mut Vec<(String, Value, Value)>,
) {
    if expected == actual {
        return;
    }
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            for key in expected
                .keys()
                .chain(actual.keys())
                .cloned()
                .collect::<BTreeSet<_>>()
            {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                diff_values(
                    &child,
                    expected.get(&key).unwrap_or(&Value::Null),
                    actual.get(&key).unwrap_or(&Value::Null),
                    changes,
                );
            }
        }
        (Value::Array(expected), Value::Array(actual)) => {
            for index in 0..expected.len().max(actual.len()) {
                diff_values(
                    &format!("{path}[{index}]"),
                    expected.get(index).unwrap_or(&Value::Null),
                    actual.get(index).unwrap_or(&Value::Null),
                    changes,
                );
            }
        }
        _ => changes.push((path.to_string(), expected.clone(), actual.clone())),
    }
}

fn markdown(report: &ComputedDiffReport) -> String {
    let mut output = format!(
        "# Computed snapshot diff\n\nMapping snapshot exact equality: `{}`\n\n| Source ID | Runtime ID | Field | Expected | Actual | Delta | Parent | Child order | Computed rectangle | Content rectangle | Transform | Camera | Stack | Clip | Visible | Border | Radius |\n| --- | --- | --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | ---: | --- | --- | --- | --- |\n",
        report.mapping_equal
    );
    for change in &report.changes {
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            escape(&change.source_id),
            escape(change.runtime_id.as_deref().unwrap_or("")),
            escape(&change.field_path),
            compact(&change.expected),
            compact(&change.actual),
            change
                .numeric_delta
                .map(|delta| delta.to_string())
                .unwrap_or_default(),
            compact(&change.parent),
            compact(&change.child_order),
            compact(&change.computed_rectangle),
            compact(&change.content_rectangle),
            compact(&change.transform),
            compact(&change.target_camera),
            compact(&change.stack_index),
            compact(&change.clipping_rectangle),
            compact(&change.visible),
            compact(&change.resolved_border),
            compact(&change.resolved_radius),
        ));
    }
    output
}

fn compact(value: &Value) -> String {
    escape(&serde_json::to_string(value).unwrap_or_else(|_| "null".into()))
}

fn escape(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_diff_is_source_and_field_ordered() {
        let expected = serde_json::json!({"width": 32.0, "x": 10.0});
        let actual = serde_json::json!({"width": 33.0, "x": 10.5});
        let mut changes = Vec::new();
        diff_values("", &expected, &actual, &mut changes);
        assert_eq!(changes[0].0, "width");
        assert_eq!(changes[1].0, "x");
        assert_eq!(
            changes[0].2.as_f64().unwrap() - changes[0].1.as_f64().unwrap(),
            1.0
        );
    }
}

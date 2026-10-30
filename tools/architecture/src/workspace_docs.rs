use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use crate::{ArchitectureError, policy::Policy, read};

/// Requires one complete ownership row per registered crate and rejects removed crate rows.
pub(super) fn check_workspace_docs(
    root: &Path,
    policy: &Policy,
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let Some(document) = &policy.workspace_document else {
        return Ok(());
    };
    let source = read(&root.join(document))?;
    let rows = source.lines().filter_map(|line| {
        let mut cells = line.trim().strip_prefix('|')?.strip_suffix('|')?.split('|');
        let path = cells.next()?.trim().strip_prefix('`')?.strip_suffix('`')?;
        let package = cells.next()?.trim().strip_prefix('`')?.strip_suffix('`')?;
        let guidance = cells.next()?.trim();
        let related = cells.next()?.trim();
        (!package.is_empty()
            && !guidance.is_empty()
            && !related.is_empty()
            && cells.next().is_none())
        .then_some((path, package))
    });
    let mut documented = BTreeMap::new();
    for (path, package) in rows {
        if documented.insert(path, package).is_some() {
            diagnostics.push(format!("{document}: duplicate ownership row for `{path}`"));
        }
    }
    let registered: BTreeSet<_> = policy
        .crate_rules
        .iter()
        .map(|rule| rule.path.trim_end_matches('/'))
        .collect();
    for path in documented.keys() {
        if !registered.contains(*path) {
            diagnostics.push(format!(
                "{document}: ownership row for `{path}` has no registered workspace crate",
            ));
        }
    }
    for rule in &policy.crate_rules {
        let Some(package) = documented.get(rule.path.trim_end_matches('/')) else {
            diagnostics.push(format!(
                "{document}: workspace crate `{}` has no complete ownership row for `{}`",
                rule.name, rule.path,
            ));
            continue;
        };
        let manifest_path = root.join(&rule.path).join("Cargo.toml");
        let manifest: toml::Value =
            toml::from_str(&read(&manifest_path)?).map_err(|source| ArchitectureError::Policy {
                path: manifest_path,
                source,
            })?;
        let expected = manifest
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
            .unwrap_or("");
        if *package != expected {
            diagnostics.push(format!(
                "{document}: ownership row for `{}` names package `{package}` instead of `{expected}`",
                rule.path,
            ));
        }
    }
    Ok(())
}

use std::{fs, path::Path};

use architecture::{ArchitectureError, check_repository};

/// Builds a two-crate repository whose ownership document is checked against its policy.
fn fixture(root: &Path, document: Option<&str>) {
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers=['alpha','beta']\n",
    )
    .expect("write workspace");
    fs::write(
        root.join("policy.toml"),
        r#"
production_rust_max = 1000
module_root_max = 300
powershell_max = 800
test_max = 1200
workspace_document = 'workspace.md'

[[crates]]
name = 'alpha'
path = 'alpha'

[[crates]]
name = 'beta'
path = 'beta'
"#,
    )
    .expect("write policy");
    for name in ["alpha", "beta"] {
        fs::create_dir(root.join(name)).expect("create crate");
        fs::write(
            root.join(name).join("Cargo.toml"),
            format!("[package]\nname='{name}'\nversion='0.1.0'\n"),
        )
        .expect("write crate manifest");
    }
    if let Some(document) = document {
        fs::write(root.join("workspace.md"), document).expect("write ownership document");
    }
}

/// Returns findings for a configured document read by the architecture crate, with no sources.
fn configured_document_findings(gate_metadata: &str, beta_metadata: &str) -> Vec<String> {
    let temp = tempfile::tempdir().expect("fixture root");
    let root = temp.path();
    let gate = env!("CARGO_PKG_NAME");
    let document = "configured.md";
    fixture(
        root,
        Some(&format!(
            "| `alpha` | `{gate}` | Check ownership. | Beta owns decoding. |\n\
            | `beta` | `beta` | Decode inputs. | The gate checks ownership. |\n"
        )),
    );
    let policy_path = root.join("policy.toml");
    let mut policy: toml::Value =
        toml::from_str(&fs::read_to_string(&policy_path).expect("read policy"))
            .expect("parse policy");
    policy["workspace_document"] = toml::Value::String(document.to_owned());
    fs::write(
        &policy_path,
        toml::to_string(&policy).expect("serialize policy"),
    )
    .expect("write configured policy");
    fs::rename(root.join("workspace.md"), root.join(document)).expect("move configured document");
    fs::write(
        root.join("alpha/Cargo.toml"),
        format!("[package]\nname='{gate}'\nversion='0.1.0'\n{gate_metadata}\n"),
    )
    .expect("write gate manifest");
    fs::write(
        root.join("beta/Cargo.toml"),
        format!("[package]\nname='beta'\nversion='0.1.0'\n{beta_metadata}\n"),
    )
    .expect("write beta manifest");
    check_repository(root, &policy_path).expect("check configured document")
}

#[test]
fn configured_document_keeps_the_architecture_read_declaration_current() {
    assert_eq!(
        configured_document_findings("[package.metadata.cinnabar]\nreads=['configured.md']", ""),
        Vec::<String>::new(),
    );
}

#[test]
fn configured_document_requires_an_architecture_read_declaration() {
    assert_eq!(
        configured_document_findings("", ""),
        [
            "alpha: policy.workspace_document reads `configured.md` outside the crate; list it in `reads` under [package.metadata.cinnabar]"
        ],
    );
}

#[test]
fn configured_document_does_not_validate_another_crates_stale_declaration() {
    let metadata = "[package.metadata.cinnabar]\nreads=['configured.md']";
    assert_eq!(
        configured_document_findings(metadata, metadata),
        ["beta: declared read `configured.md` matches nothing its sources read"],
    );
}

#[test]
fn accepts_complete_ownership_rows_in_any_order() {
    let temp = tempfile::tempdir().expect("fixture root");
    let root = temp.path();
    fixture(
        root,
        Some(
            " | `beta` | `beta` | Call beta for decoding. | Alpha owns input. |\n\
            | `alpha` | `alpha` | Call alpha for input. | Beta owns decoding. |\n",
        ),
    );
    assert_eq!(
        check_repository(root, &root.join("policy.toml")).expect("check complete guide"),
        Vec::<String>::new(),
    );
}

#[test]
fn rejects_duplicate_ownership_rows_even_when_one_package_name_is_current() {
    for packages in [
        ["stale-alpha", "alpha"],
        ["alpha", "stale-alpha"],
        ["alpha", "alpha"],
    ] {
        let temp = tempfile::tempdir().expect("fixture root");
        let root = temp.path();
        fixture(
            root,
            Some(&format!(
                "| `alpha` | `{}` | Call alpha for input. | Beta owns decoding. |\n\
                | `alpha` | `{}` | Call alpha for input. | Beta owns decoding. |\n\
                | `beta` | `beta` | Call beta for decoding. | Alpha owns input. |\n",
                packages[0], packages[1],
            )),
        );
        let findings = check_repository(root, &root.join("policy.toml"))
            .expect("check duplicate ownership rows");
        assert!(
            findings
                .iter()
                .any(|finding| finding == "workspace.md: duplicate ownership row for `alpha`"),
            "duplicate packages {packages:?} must be rejected: {findings:?}",
        );
    }
}

#[test]
fn rejects_a_crate_mentioned_only_outside_its_ownership_row() {
    let temp = tempfile::tempdir().expect("fixture root");
    let root = temp.path();
    fixture(
        root,
        Some(
            "Beta lives in `beta`.\n\
            | `alpha` | `alpha` | Call alpha for input. | `beta` owns decoding. |\n",
        ),
    );
    assert_eq!(
        check_repository(root, &root.join("policy.toml")).expect("check incomplete guide"),
        ["workspace.md: workspace crate `beta` has no complete ownership row for `beta`"],
    );
}

#[test]
fn rejects_an_ownership_row_without_caller_guidance() {
    let temp = tempfile::tempdir().expect("fixture root");
    let root = temp.path();
    fixture(
        root,
        Some(
            "| `alpha` | `alpha` | | Beta owns decoding. |\n\
            | `beta` | `beta` | Call beta for decoding. | Alpha owns input. |\n",
        ),
    );
    assert_eq!(
        check_repository(root, &root.join("policy.toml")).expect("check incomplete row"),
        ["workspace.md: workspace crate `alpha` has no complete ownership row for `alpha`"],
    );
}

#[test]
fn rejects_an_ownership_row_after_the_crate_is_removed() {
    let temp = tempfile::tempdir().expect("fixture root");
    let root = temp.path();
    fixture(
        root,
        Some(
            "| `alpha` | `alpha` | Call alpha for input. | Beta owns decoding. |\n\
            | `beta` | `beta` | Call beta for decoding. | Alpha owns input. |\n",
        ),
    );
    fs::remove_dir_all(root.join("beta")).expect("remove beta crate");
    fs::write(root.join("Cargo.toml"), "[workspace]\nmembers=['alpha']\n")
        .expect("remove beta from the workspace");
    let policy_path = root.join("policy.toml");
    let mut policy: toml::Value =
        toml::from_str(&fs::read_to_string(&policy_path).expect("read policy"))
            .expect("parse policy");
    policy["crates"]
        .as_array_mut()
        .expect("crate rules")
        .retain(|rule| rule["path"].as_str() != Some("beta"));
    fs::write(
        &policy_path,
        toml::to_string(&policy).expect("serialize policy"),
    )
    .expect("remove beta from the policy");
    assert_eq!(
        check_repository(root, &policy_path).expect("check removed crate"),
        ["workspace.md: ownership row for `beta` has no registered workspace crate"],
    );
}

#[test]
fn rejects_a_stale_package_name_after_a_rename() {
    let temp = tempfile::tempdir().expect("fixture root");
    let root = temp.path();
    fixture(
        root,
        Some(
            "| `alpha` | `alpha` | Call alpha for input. | Beta owns decoding. |\n\
            | `beta` | `beta` | Call beta for decoding. | Alpha owns input. |\n",
        ),
    );
    fs::write(
        root.join("alpha/Cargo.toml"),
        "[package]\nname='alpha-renamed'\nversion='0.1.0'\n",
    )
    .expect("rename fixture package");
    assert_eq!(
        check_repository(root, &root.join("policy.toml")).expect("check renamed package"),
        [
            "workspace.md: ownership row for `alpha` names package `alpha` instead of `alpha-renamed`"
        ],
    );
}

#[test]
fn rejects_a_missing_ownership_document() {
    let temp = tempfile::tempdir().expect("fixture root");
    let root = temp.path();
    fixture(root, None);
    let result = check_repository(root, &root.join("policy.toml"));
    assert!(
        matches!(result, Err(ArchitectureError::Read { path, .. }) if path == root.join("workspace.md"))
    );
}

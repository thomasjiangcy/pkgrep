use std::path::Path;

use crate::depspec::{self, SourceKind};

pub(super) fn run_path(cwd: &Path, dep_spec: String) -> anyhow::Result<()> {
    let parsed_specs = super::parse_dep_specs(std::slice::from_ref(&dep_spec))?;
    let spec = parsed_specs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing dependency spec"))?;

    let (locator, requested_revision) = match spec.source_kind {
        SourceKind::Git {
            url,
            requested_revision,
        } => (url, requested_revision),
        SourceKind::Registry => {
            anyhow::bail!(
                "path currently supports git-backed dependency specs only; use 'git:<url>@<revision>' or 'git:<url>#<revision>'"
            )
        }
    };

    let link_path = cwd.join(depspec::link_path(
        &spec.ecosystem,
        &locator,
        &requested_revision,
    ));

    if link_path.exists() {
        println!("{}", link_path.display());
        return Ok(());
    }

    anyhow::bail!(
        "dependency is not linked in this project: {} (expected path: {})",
        dep_spec,
        link_path.display()
    )
}

use std::path::Path;

use crate::index;

pub(super) fn run_list(cwd: &Path, json: bool) -> anyhow::Result<()> {
    let links = index::list_project_links(cwd)?;

    if json {
        let payload = serde_json::json!({
            "entries": links,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .map_err(|err| anyhow::anyhow!("failed to serialize list output: {err}"))?
        );
        return Ok(());
    }

    if links.is_empty() {
        println!("No linked dependencies found in {}", cwd.display());
        return Ok(());
    }

    for link in links {
        println!("{} -> {}", link.dep_spec, link.link_path.display());
    }

    Ok(())
}

use crate::skills;
use std::path::Path;

pub fn validate_skill_manifest(dir: &Path) -> Result<(), String> {
    let skill_md = dir.join("SKILL.md");
    if !skill_md.exists() {
        return Err(format!("Missing SKILL.md at {}", skill_md.display()));
    }
    match skills::parse_frontmatter_file(&skill_md) {
        Ok(meta) => {
            if meta.name.trim().is_empty() || meta.description.trim().is_empty() {
                Err(format!(
                    "SKILL.md missing required name/description fields at {}",
                    skill_md.display()
                ))
            } else {
                Ok(())
            }
        }
        Err(e) => Err(format!("Invalid SKILL.md at {} ({e})", skill_md.display())),
    }
}

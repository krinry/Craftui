//! Keeps the documentation honest: every fenced code block tagged `cui` in the
//! repository's Markdown files must be a complete `.cui` file that the real
//! parser, import resolver and validator accept, and that the code generator
//! can translate.
//!
//! Blocks that only show part of a file should use the `text` tag instead.

use craftui_dsl::{Severity, generate_rust, resolve_source};
use std::path::{Path, PathBuf};

/// One fenced code block: where it starts and what it contains.
struct Block {
    file: PathBuf,
    line: usize,
    source: String,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            // Build output and dependencies are not documentation.
            if !matches!(name.as_ref(), "target" | "node_modules" | "dist" | ".git") {
                collect_markdown(&path, out);
            }
        } else if name.ends_with(".md") {
            out.push(path);
        }
    }
}

fn cui_blocks(file: &Path) -> Vec<Block> {
    let text = std::fs::read_to_string(file).expect("readable markdown");
    let mut blocks = Vec::new();
    let mut current: Option<(usize, String)> = None;
    let mut in_other_fence = false;

    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        match &mut current {
            Some((start, source)) => {
                if trimmed.starts_with("```") {
                    blocks.push(Block {
                        file: file.to_path_buf(),
                        line: *start,
                        source: std::mem::take(source),
                    });
                    current = None;
                } else {
                    source.push_str(line);
                    source.push('\n');
                }
            }
            None => {
                if let Some(info) = trimmed.strip_prefix("```") {
                    if in_other_fence {
                        in_other_fence = false;
                    } else if info.trim() == "cui" {
                        current = Some((index + 2, String::new()));
                    } else {
                        in_other_fence = true;
                    }
                }
            }
        }
    }
    blocks
}

fn all_blocks() -> Vec<Block> {
    let root = repo_root();
    let mut files = Vec::new();
    collect_markdown(&root, &mut files);
    files.sort();
    files.iter().flat_map(|file| cui_blocks(file)).collect()
}

#[test]
fn every_cui_block_in_the_docs_compiles() {
    let root = repo_root();
    let blocks = all_blocks();
    let mut failures = Vec::new();

    for block in &blocks {
        let unit = resolve_source(&block.source, None);
        let errors: Vec<&str> = unit
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message.as_str())
            .collect();
        let location = format!(
            "{}:{}",
            block
                .file
                .strip_prefix(&root)
                .unwrap_or(&block.file)
                .display(),
            block.line
        );
        if !errors.is_empty() {
            failures.push(format!("{location}\n    {}", errors.join("\n    ")));
            continue;
        }
        if unit.merged.components.is_empty() {
            failures.push(format!(
                "{location}\n    the block declares no component; tag partial snippets as `text`"
            ));
            continue;
        }
        let rust = generate_rust(&unit.merged);
        if !rust.contains("pub fn build") {
            failures.push(format!(
                "{location}\n    code generation produced no builder"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} `cui` blocks in the docs are not valid:\n\n{}\n",
        failures.len(),
        blocks.len(),
        failures.join("\n\n")
    );
}

#[test]
fn the_docs_actually_contain_cui_examples() {
    // Guards the test above against passing because it found nothing to check.
    let count = all_blocks().len();
    assert!(
        count >= 15,
        "expected at least 15 `cui` examples in the docs, found {count}"
    );
}

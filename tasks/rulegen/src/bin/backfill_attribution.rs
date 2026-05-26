//! One-shot utility that walks `crates/oxc_linter/src/rules/` and adds an
//! upstream-docs `### References` section to each rule's documentation,
//! attributing the port to its upstream ESLint plugin (#22732).
//!
//! Run from the workspace root:
//!     cargo run -p rulegen --bin backfill_attribution
//!
//! The pass is idempotent: a re-run skips any rule whose doc block already
//! contains a `### References` section.
//!
//! Two cases are handled:
//!
//! 1. Rules with inline `///` doc comments inside `declare_oxc_lint!(...)`:
//!    the references section is inserted immediately before the rule's
//!    identifier line (so it renders on the docs site alongside the other
//!    sections produced from the doc comments).
//!
//! 2. Rules that pull docs from a shared `pub const DOCUMENTATION` constant
//!    (the `docs = DOCUMENTATION` form, used for jest/vitest and
//!    eslint/unicorn cross-plugin shares): the references section is appended
//!    to the constant in `crates/oxc_linter/src/rules/shared/<group>/<rule>.rs`,
//!    listing the URL for each sharing plugin.

#![expect(clippy::print_stdout, clippy::print_stderr, clippy::disallowed_methods)]

use std::{fs, path::Path};

use convert_case::{Case, Casing};
use walkdir::WalkDir;

#[derive(Clone, Copy)]
enum Plugin {
    ESLint,
    Typescript,
    Jest,
    Unicorn,
    Import,
    React,
    ReactPerf,
    JSXA11y,
    NextJS,
    JSDoc,
    Node,
    Promise,
    Vitest,
    Vue,
}

impl Plugin {
    fn from_dir(name: &str) -> Option<Self> {
        Some(match name {
            "eslint" => Self::ESLint,
            "typescript" => Self::Typescript,
            "jest" => Self::Jest,
            "unicorn" => Self::Unicorn,
            "import" => Self::Import,
            "react" => Self::React,
            "react_perf" => Self::ReactPerf,
            "jsx_a11y" => Self::JSXA11y,
            "nextjs" => Self::NextJS,
            "jsdoc" => Self::JSDoc,
            "node" => Self::Node,
            "promise" => Self::Promise,
            "vitest" => Self::Vitest,
            "vue" => Self::Vue,
            _ => return None,
        })
    }

    fn docs_url(self, kebab: &str, camel: &str) -> String {
        match self {
            Self::ESLint => format!("https://eslint.org/docs/latest/rules/{kebab}"),
            Self::Typescript => format!("https://typescript-eslint.io/rules/{kebab}/"),
            Self::Jest => format!(
                "https://github.com/jest-community/eslint-plugin-jest/blob/main/docs/rules/{kebab}.md"
            ),
            Self::Unicorn => format!(
                "https://github.com/sindresorhus/eslint-plugin-unicorn/blob/main/docs/rules/{kebab}.md"
            ),
            Self::Import => format!(
                "https://github.com/import-js/eslint-plugin-import/blob/main/docs/rules/{kebab}.md"
            ),
            Self::React => format!(
                "https://github.com/jsx-eslint/eslint-plugin-react/blob/master/docs/rules/{kebab}.md"
            ),
            Self::ReactPerf => format!(
                "https://github.com/cvazac/eslint-plugin-react-perf/blob/master/docs/rules/{kebab}.md"
            ),
            Self::JSXA11y => format!(
                "https://github.com/jsx-eslint/eslint-plugin-jsx-a11y/blob/main/docs/rules/{kebab}.md"
            ),
            Self::NextJS => format!("https://nextjs.org/docs/messages/{kebab}"),
            Self::JSDoc => format!(
                "https://github.com/gajus/eslint-plugin-jsdoc/blob/main/docs/rules/{camel}.md"
            ),
            Self::Node => format!(
                "https://github.com/eslint-community/eslint-plugin-n/blob/master/docs/rules/{kebab}.md"
            ),
            Self::Promise => format!(
                "https://github.com/eslint-community/eslint-plugin-promise/blob/main/docs/rules/{kebab}.md"
            ),
            Self::Vitest => format!(
                "https://github.com/vitest-dev/eslint-plugin-vitest/blob/main/docs/rules/{kebab}.md"
            ),
            Self::Vue => format!("https://eslint.vuejs.org/rules/{kebab}.html"),
        }
    }
}

/// Plugins that share docs via a constant under `crates/oxc_linter/src/rules/shared/<group>/`.
fn shared_group_plugins(group: &str) -> Option<&'static [Plugin]> {
    Some(match group {
        "jest_vitest" => &[Plugin::Jest, Plugin::Vitest],
        "eslint_unicorn" => &[Plugin::ESLint, Plugin::Unicorn],
        _ => return None,
    })
}

/// Derive the rule's kebab-case name from its file path.
///
/// - `.../<rule>.rs`        → uses the file stem
/// - `.../<rule>/mod.rs`    → uses the parent directory name
fn rule_name_from_path(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    if stem == "mod" {
        path.parent()?.file_name()?.to_str().map(str::to_owned)
    } else {
        Some(stem.to_owned())
    }
}

/// Insert a `### References` block inside the `declare_oxc_lint!(...)` doc
/// comments, immediately after the last `///` line and before the rule's
/// identifier. Returns `None` if the file has no inline-doc macro block
/// (e.g. uses `docs = CONSTANT`) or already contains a references section.
fn insert_references_in_inline_docs(contents: &str, url: &str) -> Option<String> {
    let macro_pat = "declare_oxc_lint!(";
    let macro_start = contents.find(macro_pat)?;
    let body_start = macro_start + macro_pat.len();

    let mut cursor = body_start;
    let mut last_doc_line_end: Option<usize> = None;
    let mut saw_references = false;

    while cursor < contents.len() {
        let line_end = match contents[cursor..].find('\n') {
            Some(n) => cursor + n + 1,
            None => contents.len(),
        };
        let line = &contents[cursor..line_end];
        let trimmed = line.trim_start();
        if trimmed.starts_with("///") {
            if trimmed.contains("### References") {
                saw_references = true;
            }
            last_doc_line_end = Some(line_end);
        } else if trimmed.is_empty() {
            // Tolerate blank lines between doc and the identifier (rare).
        } else {
            break;
        }
        cursor = line_end;
    }

    if saw_references {
        return None;
    }

    let insertion_point = last_doc_line_end?;
    let block = format!("    ///\n    /// ### References\n    ///\n    /// - <{url}>\n");

    let mut new = String::with_capacity(contents.len() + block.len());
    new.push_str(&contents[..insertion_point]);
    new.push_str(&block);
    new.push_str(&contents[insertion_point..]);
    Some(new)
}

/// Append a `### References` section to a shared `pub const DOCUMENTATION` raw
/// string. Returns `None` if the file has no such constant or already contains
/// a references section.
fn append_references_to_shared_doc(contents: &str, urls: &[String]) -> Option<String> {
    let const_pat = "pub const DOCUMENTATION: &str = r\"";
    let const_start = contents.find(const_pat)?;
    let body_start = const_start + const_pat.len();
    // Find the closing `";` after `body_start`.
    let body_end_offset = contents[body_start..].find("\";")?;
    let body_end = body_start + body_end_offset;

    let body = &contents[body_start..body_end];
    if body.contains("### References") {
        return None;
    }

    // Compose the block. Preserve a trailing newline before the `";` if present.
    let needs_leading_nl = !body.ends_with('\n');
    let needs_blank_separator = !body.ends_with("\n\n") && !body.is_empty();

    let mut block = String::new();
    if needs_leading_nl {
        block.push('\n');
    }
    if needs_blank_separator {
        block.push('\n');
    }
    block.push_str("### References\n\n");
    for url in urls {
        block.push_str(&format!("- <{url}>\n"));
    }

    let mut new = String::with_capacity(contents.len() + block.len());
    new.push_str(&contents[..body_end]);
    new.push_str(&block);
    new.push_str(&contents[body_end..]);
    Some(new)
}

fn process_plugin_rule(path: &Path, plugin: Plugin) -> Result<bool, std::io::Error> {
    let contents = fs::read_to_string(path)?;
    // Rule files that use `docs = CONSTANT` defer their docs to the shared constant
    // file; we attribute those in the shared pass.
    if !contents.contains("declare_oxc_lint!(\n") && contents.contains("docs = ") {
        return Ok(false);
    }
    let Some(rule_name) = rule_name_from_path(path) else {
        return Ok(false);
    };
    let kebab = rule_name.to_case(Case::Kebab);
    let camel = rule_name.to_case(Case::Camel);
    let url = plugin.docs_url(&kebab, &camel);
    if let Some(new_contents) = insert_references_in_inline_docs(&contents, &url) {
        fs::write(path, &new_contents)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn process_shared_rule(path: &Path, plugins: &[Plugin]) -> Result<bool, std::io::Error> {
    let contents = fs::read_to_string(path)?;
    let Some(rule_name) = rule_name_from_path(path) else {
        return Ok(false);
    };
    let kebab = rule_name.to_case(Case::Kebab);
    let camel = rule_name.to_case(Case::Camel);
    let urls: Vec<String> = plugins.iter().map(|p| p.docs_url(&kebab, &camel)).collect();
    if let Some(new_contents) = append_references_to_shared_doc(&contents, &urls) {
        fs::write(path, &new_contents)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn main() {
    let rules_dir = Path::new("crates/oxc_linter/src/rules");
    if !rules_dir.is_dir() {
        eprintln!("error: run from the workspace root; '{}' not found", rules_dir.display());
        std::process::exit(1);
    }

    let mut changed = 0usize;
    let mut skipped = 0usize;

    for entry in WalkDir::new(rules_dir).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }

        let rel = match path.strip_prefix(rules_dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let Some(first) = rel.iter().next().and_then(|s| s.to_str()) else {
            continue;
        };

        if first == "shared" {
            // shared/<group>/<rule>.rs
            let Some(group) = rel.iter().nth(1).and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(plugins) = shared_group_plugins(group) else {
                continue;
            };
            if path.file_name().and_then(|s| s.to_str()) == Some("mod.rs") {
                continue;
            }
            match process_shared_rule(path, plugins) {
                Ok(true) => changed += 1,
                Ok(false) => skipped += 1,
                Err(e) => eprintln!("error processing {}: {e}", path.display()),
            }
        } else if let Some(plugin) = Plugin::from_dir(first) {
            match process_plugin_rule(path, plugin) {
                Ok(true) => changed += 1,
                Ok(false) => skipped += 1,
                Err(e) => eprintln!("error processing {}: {e}", path.display()),
            }
        }
        // `oxc` (native) and unknown dirs are skipped.
    }

    println!("Done. {changed} files updated, {skipped} skipped.");
}

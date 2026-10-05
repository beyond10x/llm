//! Checks over the hand-written pages.
//!
//! - A code block whose fence carries `title="crates/llm-docs/examples/<name>.rs"` must equal that
//!   file byte for byte. The examples are compiled by the gate and run by hand before a guide
//!   quotes their output, so a guide cannot drift from code that builds.
//! - No admonition carries a raw title (`:::caution Planned`): Docusaurus prints those as text.

use std::{fs, path::Path};

use crate::{DOCS, Result};

/// The directory whose files a guide may quote whole.
const EXAMPLES: &str = "crates/llm-docs/examples/";

/// Whether `line` matches `^:::[a-z]+ +\S`: an admonition whose title is not in brackets.
fn is_raw_admonition(line: &str) -> bool {
    let Some(rest) = line.strip_prefix(":::") else {
        return false;
    };
    let kind = rest.bytes().take_while(u8::is_ascii_lowercase).count();
    let rest = &rest[kind..];
    let spaces = rest.bytes().take_while(|b| *b == b' ').count();
    kind > 0
        && spaces > 0
        && rest[spaces..]
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace())
}

/// The example file a fence line quotes, if any.
fn quoted_example(fence: &str) -> Option<&str> {
    let start = fence.find("title=\"")? + "title=\"".len();
    let path = &fence[start..];
    let path = &path[..path.find('"')?];
    path.starts_with(EXAMPLES).then_some(path)
}

/// Every problem in one page: `page:line: problem`.
fn page_problems(root: &Path, name: &str, page: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let lines: Vec<&str> = page.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if is_raw_admonition(line) {
            problems.push(format!(
                "{name}:{}: raw admonition title (write `:::kind[Title]`)",
                index + 1
            ));
        }
        if let Some(fence) = line.strip_prefix("```") {
            let opened = index;
            let end = lines[index + 1..]
                .iter()
                .position(|candidate| candidate.trim_end() == "```")
                .map(|offset| index + 1 + offset);
            let Some(end) = end else {
                problems.push(format!("{name}:{}: unclosed code block", opened + 1));
                break;
            };
            if let Some(example) = quoted_example(fence) {
                let block = lines[opened + 1..end].join("\n") + "\n";
                match fs::read_to_string(root.join(example)) {
                    Err(_) => problems.push(format!(
                        "{name}:{}: quotes {example}, which does not exist",
                        opened + 1
                    )),
                    Ok(file) if file != block => problems.push(format!(
                        "{name}:{}: block differs from {example}",
                        opened + 1
                    )),
                    Ok(_) => {}
                }
            }
            index = end;
        }
        index += 1;
    }
    problems
}

fn walk(root: &Path, dir: &Path, problems: &mut Vec<String>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|error| format!("reading {}: {error}", dir.display()))?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(root, &path, problems)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext == "md" || ext == "mdx")
        {
            let page = fs::read_to_string(&path)
                .map_err(|error| format!("reading {}: {error}", path.display()))?;
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            problems.extend(page_problems(root, &name, &page));
        }
    }
    Ok(())
}

/// Every problem in the pages under `website/docs`.
pub(crate) fn check(root: &Path) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    walk(root, &root.join(DOCS), &mut problems)?;
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("llm-docs-pages-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(EXAMPLES)).unwrap();
        dir
    }

    #[test]
    fn raw_admonition_titles_are_found_and_bracketed_ones_pass() {
        for raw in [
            ":::caution Planned",
            ":::note  These",
            ":::info Decided design",
        ] {
            assert!(is_raw_admonition(raw), "{raw}");
        }
        for fine in [
            ":::caution[Planned]",
            ":::",
            ":::note",
            ":::note ",
            "::: note T",
            " :::note T",
        ] {
            assert!(!is_raw_admonition(fine), "{fine}");
        }
    }

    #[test]
    fn quoted_examples_must_equal_their_files() {
        let root = scratch("examples");
        fs::write(root.join(EXAMPLES).join("a.rs"), "fn main() {}\n").unwrap();
        let page =
            |body: &str| format!("# Guide\n\n```rust title=\"{EXAMPLES}a.rs\"\n{body}\n```\n");
        assert!(page_problems(&root, "g.md", &page("fn main() {}")).is_empty());
        let edited = page_problems(&root, "g.md", &page("fn main() { edited }"));
        assert_eq!(
            edited,
            [format!("g.md:3: block differs from {EXAMPLES}a.rs")]
        );
        let missing = page_problems(
            &root,
            "g.md",
            &format!("```rust title=\"{EXAMPLES}b.rs\"\nx\n```\n"),
        );
        assert_eq!(
            missing,
            [format!(
                "g.md:1: quotes {EXAMPLES}b.rs, which does not exist"
            )]
        );
        let other = page_problems(
            &root,
            "g.md",
            "```rust title=\"src/lib.rs\"\nanything\n```\n",
        );
        assert!(other.is_empty(), "only example files are checked");
        let raw = page_problems(
            &root,
            "g.md",
            "```text\n:::note Inside a block\n```\n:::note Outside\n",
        );
        assert_eq!(
            raw,
            ["g.md:4: raw admonition title (write `:::kind[Title]`)"]
        );
        fs::remove_dir_all(root).unwrap();
    }
}

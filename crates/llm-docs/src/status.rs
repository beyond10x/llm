//! The status data and page, from `docs/implementation-status.md`.
//!
//! The source's status section opens with `## Status at <version> (<YYYY-MM-DD>)`. Each `###`
//! heading inside it names an area, and each table row under it is one capability:
//!
//! ```text
//! | `story-id` | **Label.** What exists and its evidence. |
//! | `story-id` | Pending: **Label.** What is missing. |
//! ```
//!
//! A row whose text opens with `Pending` is `planned`; every other row is `shipped`. The label is
//! the bold sentence (or, without bold, the first sentence), without its full stop; the detail is
//! the rest, as plain text, and is left out when there is none.

use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::{MDX_HEADER, Result, STATUS_SOURCE, yaml_string};

/// The heading that opens the status section.
const SECTION: &str = "## Status at ";

pub struct Item {
    pub area: String,
    pub label: String,
    pub detail: Option<String>,
    pub shipped: bool,
}

pub struct Record {
    pub version: String,
    pub as_of: String,
    pub items: Vec<Item>,
}

fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        })
}

/// Markdown inline markup removed, for a plain-text table cell.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        // A link `[text](target)` keeps its text.
        let Some(close) = rest[start..].find("](") else {
            break;
        };
        let Some(end) = rest[start + close..].find(')') else {
            break;
        };
        out.push_str(&rest[..start]);
        out.push_str(&rest[start + 1..start + close]);
        rest = &rest[start + close + end + 1..];
    }
    out.push_str(rest);
    out.replace("**", "").replace('`', "").replace("\\|", "|")
}

fn row(line: &str, area: &str, number: usize) -> Result<Option<Item>> {
    let Some(rest) = line.strip_prefix("| `") else {
        return Ok(None);
    };
    let at = |problem: &str| format!("{STATUS_SOURCE}:{number}: {problem}");
    let (_, rest) = rest
        .split_once("` |")
        .ok_or_else(|| at("the first cell is not one code span"))?;
    let text = rest
        .trim()
        .strip_suffix('|')
        .ok_or_else(|| at("the row does not end with |"))?;
    let text = text.trim();
    if area.is_empty() {
        return Err(at("the row is not under an area heading (###)"));
    }
    let (shipped, text) = match text.strip_prefix("Pending") {
        Some(rest) => (false, rest.trim_start_matches(':').trim_start()),
        None => (true, text),
    };
    // `**Label.** detail`, or a bare `Label. detail` whose first sentence is the label.
    let (label, detail) = match text.strip_prefix("**") {
        Some(body) => body
            .split_once("**")
            .ok_or_else(|| at("the bold label is not closed"))?,
        None => text.split_once(". ").unwrap_or((text, "")),
    };
    let label = label.trim().trim_end_matches('.').trim();
    if label.is_empty() {
        return Err(at("the label is empty"));
    }
    let detail = plain(detail.trim());
    Ok(Some(Item {
        area: area.to_owned(),
        label: plain(label),
        detail: (!detail.is_empty()).then_some(detail),
        shipped,
    }))
}

/// Read the status section of the source.
pub(crate) fn parse(source: &str) -> Result<Record> {
    let mut lines = source.lines().enumerate();
    let heading = lines
        .by_ref()
        .find_map(|(_, line)| line.strip_prefix(SECTION))
        .ok_or_else(|| format!("{STATUS_SOURCE} has no `{SECTION}<version> (<date>)` heading"))?;
    let (version, date) = heading
        .split_once(" (")
        .and_then(|(version, rest)| Some((version.trim(), rest.strip_suffix(')')?)))
        .filter(|(version, date)| !version.is_empty() && is_date(date))
        .ok_or_else(|| {
            format!("{STATUS_SOURCE}: `{SECTION}{heading}` is not `<version> (YYYY-MM-DD)`")
        })?;
    let mut area = String::new();
    let mut items: Vec<Item> = Vec::new();
    for (index, line) in lines {
        if line.starts_with("## ") {
            break;
        }
        if let Some(name) = line.strip_prefix("### ") {
            name.trim().clone_into(&mut area);
            continue;
        }
        if let Some(item) = row(line, &area, index + 1)? {
            if items.iter().any(|seen| seen.label == item.label) {
                return Err(format!(
                    "{STATUS_SOURCE}:{}: the label {:?} is listed twice",
                    index + 1,
                    item.label
                ));
            }
            items.push(item);
        }
    }
    if items.is_empty() {
        return Err(format!("{STATUS_SOURCE} lists no capability"));
    }
    Ok(Record {
        version: version.to_owned(),
        as_of: date.to_owned(),
        items,
    })
}

/// `website/data/status.json`, a `b10x-status/1` document.
pub(crate) fn data(record: &Record) -> String {
    let items: Vec<Value> = record
        .items
        .iter()
        .map(|item| {
            let mut value = json!({
                "area": item.area,
                "label": item.label,
                "status": if item.shipped { "shipped" } else { "planned" },
            });
            if let Some(detail) = &item.detail {
                value["detail"] = json!(detail);
            }
            value
        })
        .collect();
    let document = json!({
        "format": "b10x-status/1",
        "asOf": record.as_of,
        "source": format!("{STATUS_SOURCE} in the llm repository, at {}", record.version),
        "items": items,
    });
    serde_json::to_string_pretty(&document).expect("a JSON value serializes") + "\n"
}

/// `website/docs/status.mdx`.
pub(crate) fn page(record: &Record) -> String {
    let shipped = record.items.iter().filter(|item| item.shipped).count();
    let planned = record.items.len() - shipped;
    let version = &record.version;
    let mut page = format!(
        "---\ntitle: Status\nsidebar_position: 1\ndescription: {}\nlede: {}\nsource: {}\nsource_url: https://github.com/beyond10x/llm/blob/main/{STATUS_SOURCE}\ncustom_edit_url: null\n---\n\n{MDX_HEADER}\n\n",
        yaml_string(&format!(
            "What llm {version} ships and what is planned, capability by capability."
        )),
        yaml_string(&format!(
            "{shipped} capabilities ship in llm {version}; {planned} are planned. The landing page reads the same file."
        )),
        yaml_string(&format!(
            "{STATUS_SOURCE}, generated into website/data/status.json"
        )),
    );
    page.push_str("import status from '@site/data/status.json';\n\n");
    let _ = write!(
        page,
        "**Shipped** means in llm's libraries at {version}, tested in the repository gate. \
         **Planned** means on the plan and not built. The gate runs against recorded response bytes, \
         loopback sockets and in-process fakes, and makes no paid provider call, so *shipped* is \
         not the same as *qualified against a live provider*: [Limitations](./status/limitations.md) \
         says what the evidence does not establish, and [Not yet](./status/roadmap.md) what each \
         planned item waits for.\n\n"
    );
    page.push_str("<StatusTable data={status} />\n");
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "# Status\n\nIntro.\n\n## Status at 0.1.7 (2026-10-05)\n\n\
        ### Protocols\n\n| Story | Capability |\n| --- | --- |\n\
        | `a` | **The `A` client.** Ships `A` ([doc](a.md)). |\n\
        | `b` | Pending: **B access.** Not \\| yet. |\n\n\
        ### Accounting\n\n| `c` | **Pricing.** Exact. |\n| `e` | Pending: Bare row. |\n\n## Next\n\n| `d` | **Ignored.** x |\n";

    #[test]
    fn rows_become_items_with_area_label_detail_and_status() {
        let record = parse(SOURCE).unwrap();
        assert_eq!(record.version, "0.1.7");
        assert_eq!(record.as_of, "2026-10-05");
        let items: Vec<_> = record
            .items
            .iter()
            .map(|item| {
                (
                    item.area.as_str(),
                    item.label.as_str(),
                    item.detail.as_deref(),
                    item.shipped,
                )
            })
            .collect();
        assert_eq!(
            items,
            [
                ("Protocols", "The A client", Some("Ships A (doc)."), true),
                ("Protocols", "B access", Some("Not | yet."), false),
                ("Accounting", "Pricing", Some("Exact."), true),
                ("Accounting", "Bare row", None, false),
            ]
        );
        let data: Value = serde_json::from_str(&data(&record)).unwrap();
        assert_eq!(data["format"], "b10x-status/1");
        assert_eq!(data["asOf"], "2026-10-05");
        assert_eq!(data["items"][1]["status"], "planned");
        assert!(
            data["items"][3].get("detail").is_none(),
            "no detail, no key"
        );
        let page = page(&record);
        assert!(page.contains(MDX_HEADER));
        assert!(page.contains("<StatusTable data={status} />"));
        assert!(page.contains("2 capabilities ship in llm 0.1.7; 2 are planned."));
    }

    #[test]
    fn malformed_sources_are_refused_with_their_line() {
        let cases = [
            ("# No section\n", "heading"),
            ("## Status at 0.1.7 (5 Oct)\n", "YYYY-MM-DD"),
            (
                "## Status at 0.1.7 (2026-10-05)\n| `a` | **A.** x |\n",
                "area heading",
            ),
            (
                "## Status at 0.1.7 (2026-10-05)\n### X\n| `a` | **.** x |\n",
                "label is empty",
            ),
            (
                "## Status at 0.1.7 (2026-10-05)\n### X\n| `a` | **A. x |\n",
                "not closed",
            ),
            (
                "## Status at 0.1.7 (2026-10-05)\n### X\n| `a` | **A.** x |\n| `b` | **A** y |\n",
                "twice",
            ),
            ("## Status at 0.1.7 (2026-10-05)\n### X\n", "no capability"),
        ];
        for (source, expected) in cases {
            let error = parse(source).err().unwrap_or_default();
            assert!(error.contains(expected), "{source:?}: {error}");
        }
        let error =
            parse("## Status at 1 (2026-10-05)\n### X\n\n| `a` | **A.** x |\n| `b` | **A.** y |\n")
                .err()
                .unwrap();
        assert!(
            error.contains(":5:"),
            "the line number is the row's: {error}"
        );
    }
}

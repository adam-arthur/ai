use anyhow::{Context, Result, bail};
use ego_tree::NodeRef;
use scraper::{ElementRef, Html, Node, Selector};

const ALLOWED_ELEMENTS: &[&str] = &[
    "a", "b", "br", "caption", "col", "colgroup", "div", "em", "font", "i", "p", "s", "small",
    "span", "strong", "sub", "sup", "table", "tbody", "td", "tfoot", "th", "thead", "tr", "u",
];

const PRESENTATIONAL_ATTRIBUTES: &[&str] = &[
    "align",
    "bgcolor",
    "border",
    "cellpadding",
    "cellspacing",
    "class",
    "face",
    "height",
    "nowrap",
    "size",
    "style",
    "valign",
    "width",
];

const NON_DATA_ATTRIBUTES: &[&str] = &["href"];

const PRESERVED_ATTRIBUTES: &[&str] = &[
    "abbr", "colspan", "dir", "headers", "id", "lang", "rowspan", "scope", "title",
];

const STRUCTURAL_ELEMENTS: &[&str] = &["table", "colgroup", "thead", "tbody", "tfoot", "tr"];

#[derive(Debug, Eq, PartialEq)]
enum SemanticToken {
    Start(String, Vec<(String, String)>),
    End(String),
    Text(String),
    Comment(String),
}

pub(super) fn clean_table_html(table_html: &str) -> Result<String> {
    let cleaned = clean_once(table_html)?;
    let cleaned_again = clean_once(&cleaned)?;
    if cleaned != cleaned_again {
        bail!("table HTML cleanup is not idempotent");
    }
    Ok(cleaned)
}

fn clean_once(table_html: &str) -> Result<String> {
    let mut document = Html::parse_fragment(table_html);
    validate(&document)?;
    let before = semantic_projection(&document)?;

    normalize_nonbreaking_spaces(&mut document);
    remove_non_data_attributes(&mut document);
    unwrap_fonts(&mut document);
    empty_whitespace_only_cells(&mut document);
    remove_structural_whitespace(&mut document);

    let cleaned = pretty_table_html(&document)?;
    let reparsed = Html::parse_fragment(&cleaned);
    validate(&reparsed)?;
    let after = semantic_projection(&reparsed)?;
    if before != after {
        bail!("table HTML cleanup changed table structure or semantic content");
    }
    Ok(cleaned)
}

fn normalize_nonbreaking_spaces(document: &mut Html) {
    let text_ids = document
        .tree
        .nodes()
        .filter(|node| {
            node.value()
                .as_text()
                .is_some_and(|text| text.contains('\u{a0}'))
        })
        .map(|node| node.id())
        .collect::<Vec<_>>();

    for id in text_ids {
        let mut node = document
            .tree
            .get_mut(id)
            .expect("text node remains in tree");
        if let Node::Text(text) = node.value() {
            text.text = text.replace('\u{a0}', " ").into();
        }
    }
}

fn empty_whitespace_only_cells(document: &mut Html) {
    let whitespace_child_ids = document
        .tree
        .nodes()
        .filter(|node| {
            node.value()
                .as_text()
                .is_some_and(|text| text.trim().is_empty())
                && node.parent().is_some_and(|parent| {
                    parent.value().as_element().is_some_and(|element| {
                        matches!(element.name(), "td" | "th")
                            && parent.children().all(|child| child.value().is_text())
                    })
                })
        })
        .map(|node| node.id())
        .collect::<Vec<_>>();

    for id in whitespace_child_ids {
        document
            .tree
            .get_mut(id)
            .expect("whitespace node remains in tree")
            .detach();
    }
}

fn pretty_table_html(document: &Html) -> Result<String> {
    let table = only_table(document)?;
    let mut output = String::new();
    write_pretty_node(
        document
            .tree
            .get(table.id())
            .context("table disappeared from parsed HTML")?,
        0,
        &mut output,
    );
    Ok(output)
}

fn write_pretty_node(node: NodeRef<'_, Node>, depth: usize, output: &mut String) {
    let Node::Element(element) = node.value() else {
        write_indent(depth, output);
        write_compact_node(node, output);
        return;
    };

    write_indent(depth, output);
    write_start_tag(element, output);
    let name = element.name();
    if matches!(name, "br" | "col") {
        return;
    }

    let children = node.children().collect::<Vec<_>>();
    if children.is_empty() {
        output.push_str("</");
        output.push_str(name);
        output.push('>');
        return;
    }

    if STRUCTURAL_ELEMENTS.contains(&name) {
        for child in children {
            output.push('\n');
            if child
                .value()
                .as_element()
                .is_some_and(|child| STRUCTURAL_ELEMENTS.contains(&child.name()))
            {
                write_pretty_node(child, depth + 1, output);
            } else {
                write_indent(depth + 1, output);
                write_compact_node(child, output);
            }
        }
        output.push('\n');
        write_indent(depth, output);
    } else {
        for child in children {
            write_compact_node(child, output);
        }
    }
    output.push_str("</");
    output.push_str(name);
    output.push('>');
}

fn write_compact_node(node: NodeRef<'_, Node>, output: &mut String) {
    match node.value() {
        Node::Element(element) => {
            write_start_tag(element, output);
            if !matches!(element.name(), "br" | "col") {
                for child in node.children() {
                    write_compact_node(child, output);
                }
                output.push_str("</");
                output.push_str(element.name());
                output.push('>');
            }
        }
        Node::Text(text) => write_escaped(text, output, false),
        Node::Comment(comment) => {
            output.push_str("<!--");
            output.push_str(comment);
            output.push_str("-->");
        }
        Node::Document | Node::Fragment => {
            for child in node.children() {
                write_compact_node(child, output);
            }
        }
        Node::Doctype(_) | Node::ProcessingInstruction(_) => {}
    }
}

fn write_start_tag(element: &scraper::node::Element, output: &mut String) {
    output.push('<');
    output.push_str(element.name());
    for (name, value) in element.attrs() {
        output.push(' ');
        output.push_str(name);
        output.push_str("=\"");
        write_escaped(value, output, true);
        output.push('"');
    }
    output.push('>');
}

fn write_escaped(value: &str, output: &mut String, attribute: bool) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' if attribute => output.push_str("&quot;"),
            _ => output.push(character),
        }
    }
}

fn write_indent(depth: usize, output: &mut String) {
    for _ in 0..depth {
        output.push_str("  ");
    }
}

fn validate(document: &Html) -> Result<()> {
    let table = only_table(document)?;
    for node in table.descendants() {
        let Some(element) = node.value().as_element() else {
            continue;
        };
        let name = element.name();
        if !ALLOWED_ELEMENTS.contains(&name) {
            bail!("unsupported table element <{name}>");
        }
        for (attribute, _) in element.attrs() {
            if !PRESENTATIONAL_ATTRIBUTES.contains(&attribute)
                && !NON_DATA_ATTRIBUTES.contains(&attribute)
                && !PRESERVED_ATTRIBUTES.contains(&attribute)
            {
                bail!("unsupported attribute {attribute:?} on <{name}>");
            }
        }
    }
    Ok(())
}

fn only_table(document: &Html) -> Result<ElementRef<'_>> {
    let selector = Selector::parse("table").expect("valid table selector");
    let mut tables = document.select(&selector);
    let table = tables.next().context("HTML fragment has no table")?;
    if tables.next().is_some() {
        bail!("HTML fragment contains multiple or nested tables");
    }

    let root = document.tree.root();
    for node in root.descendants() {
        if node.id() == table.id() || node.ancestors().any(|ancestor| ancestor.id() == table.id()) {
            continue;
        }
        match node.value() {
            Node::Text(text) if !text.trim().is_empty() => {
                bail!("HTML fragment contains text outside the table");
            }
            Node::Element(element) if !matches!(element.name(), "html" | "body") => {
                bail!("HTML fragment contains an element outside the table");
            }
            Node::Comment(_) | Node::Doctype(_) | Node::ProcessingInstruction(_) => {
                bail!("HTML fragment contains non-table markup outside the table");
            }
            _ => {}
        }
    }
    Ok(table)
}

fn remove_non_data_attributes(document: &mut Html) {
    let element_ids = document
        .tree
        .nodes()
        .filter(|node| node.value().is_element())
        .map(|node| node.id())
        .collect::<Vec<_>>();

    for id in element_ids {
        let mut node = document.tree.get_mut(id).expect("node remains in tree");
        if let Node::Element(element) = node.value() {
            element.attrs.retain(|(name, _)| {
                !PRESENTATIONAL_ATTRIBUTES.contains(&name.local.as_ref())
                    && !NON_DATA_ATTRIBUTES.contains(&name.local.as_ref())
            });
        }
    }
}

fn unwrap_fonts(document: &mut Html) {
    let font_ids = document
        .tree
        .nodes()
        .filter(|node| {
            node.value()
                .as_element()
                .is_some_and(|element| element.name() == "font")
        })
        .map(|node| node.id())
        .collect::<Vec<_>>();

    // Innermost-first keeps nested font contents in their original order.
    for font_id in font_ids.into_iter().rev() {
        let child_ids = document
            .tree
            .get(font_id)
            .expect("font remains in tree")
            .children()
            .map(|child| child.id())
            .collect::<Vec<_>>();
        for child_id in child_ids {
            document
                .tree
                .get_mut(font_id)
                .expect("font remains in tree")
                .insert_id_before(child_id);
        }
        document
            .tree
            .get_mut(font_id)
            .expect("font remains in tree")
            .detach();
    }
}

fn remove_structural_whitespace(document: &mut Html) {
    let whitespace_ids = document
        .tree
        .nodes()
        .filter(|node| {
            node.value()
                .as_text()
                .is_some_and(|text| text.trim().is_empty())
                && node.parent().is_some_and(|parent| {
                    parent
                        .value()
                        .as_element()
                        .is_some_and(|element| STRUCTURAL_ELEMENTS.contains(&element.name()))
                })
        })
        .map(|node| node.id())
        .collect::<Vec<_>>();

    for id in whitespace_ids {
        document
            .tree
            .get_mut(id)
            .expect("whitespace node remains in tree")
            .detach();
    }
}

fn semantic_projection(document: &Html) -> Result<Vec<SemanticToken>> {
    let table = only_table(document)?;
    let mut projection = Vec::new();
    project_node(
        document
            .tree
            .get(table.id())
            .context("table disappeared from parsed HTML")?,
        &mut projection,
    );
    let whitespace_only_cell_text = (1..projection.len().saturating_sub(1))
        .filter(|&index| {
            matches!(&projection[index], SemanticToken::Text(text) if text.trim().is_empty())
                && matches!(
                    (&projection[index - 1], &projection[index + 1]),
                    (SemanticToken::Start(start, _), SemanticToken::End(end))
                        if start == end && matches!(start.as_str(), "td" | "th")
                )
        })
        .collect::<Vec<_>>();
    for index in whitespace_only_cell_text.into_iter().rev() {
        projection.remove(index);
    }
    Ok(projection)
}

fn project_node(node: NodeRef<'_, Node>, projection: &mut Vec<SemanticToken>) {
    match node.value() {
        Node::Element(element) if element.name() == "font" => {
            for child in node.children() {
                project_node(child, projection);
            }
        }
        Node::Element(element) => {
            let name = element.name().to_owned();
            let mut attributes = element
                .attrs()
                .filter(|(attribute, _)| {
                    !PRESENTATIONAL_ATTRIBUTES.contains(attribute)
                        && !NON_DATA_ATTRIBUTES.contains(attribute)
                })
                .map(|(attribute, value)| (attribute.to_owned(), value.to_owned()))
                .collect::<Vec<_>>();
            attributes.sort();
            projection.push(SemanticToken::Start(name.clone(), attributes));
            for child in node.children() {
                if STRUCTURAL_ELEMENTS.contains(&name.as_str())
                    && child
                        .value()
                        .as_text()
                        .is_some_and(|text| text.trim().is_empty())
                {
                    continue;
                }
                project_node(child, projection);
            }
            projection.push(SemanticToken::End(name));
        }
        Node::Text(text) => {
            let normalized = text.replace('\u{a0}', " ");
            if let Some(SemanticToken::Text(previous)) = projection.last_mut() {
                previous.push_str(&normalized);
            } else {
                projection.push(SemanticToken::Text(normalized));
            }
        }
        Node::Comment(comment) => projection.push(SemanticToken::Comment(comment.to_string())),
        Node::Document | Node::Fragment => {
            for child in node.children() {
                project_node(child, projection);
            }
        }
        Node::Doctype(_) | Node::ProcessingInstruction(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_only_presentational_markup() {
        let input = r#"
            <table cellpadding="0" style="width: 90%">
              <tr bgcolor="white">
                <td colspan="2" style="text-align: right"><font face="Arial"><b>FFO</b></font><br>$&nbsp;42</td>
              </tr>
            </table>
        "#;

        assert_eq!(
            clean_table_html(input).unwrap(),
            concat!(
                "<table>\n",
                "  <tbody>\n",
                "    <tr>\n",
                "      <td colspan=\"2\"><b>FFO</b><br>$ 42</td>\n",
                "    </tr>\n",
                "  </tbody>\n",
                "</table>"
            )
        );
    }

    #[test]
    fn normalizes_cell_whitespace_and_preserves_accounting_columns() {
        let input = "<table><tr><td>$</td><td> (79,104</td><td>)</td><td><font>&nbsp;</font></td></tr></table>";

        assert_eq!(
            clean_table_html(input).unwrap(),
            concat!(
                "<table>\n",
                "  <tbody>\n",
                "    <tr>\n",
                "      <td>$</td>\n",
                "      <td> (79,104</td>\n",
                "      <td>)</td>\n",
                "      <td></td>\n",
                "    </tr>\n",
                "  </tbody>\n",
                "</table>"
            )
        );
    }

    #[test]
    fn unwraps_adjacent_fonts_without_changing_text() {
        let input = "<table><tr><td><font>For the </font><font>year ended</font></td></tr></table>";

        assert_eq!(
            clean_table_html(input).unwrap(),
            concat!(
                "<table>\n",
                "  <tbody>\n",
                "    <tr>\n",
                "      <td>For the year ended</td>\n",
                "    </tr>\n",
                "  </tbody>\n",
                "</table>"
            )
        );
    }

    #[test]
    fn removes_link_destinations_but_preserves_link_text() {
        let input = r##"<table><tr><td><a href="#details">FFO details</a></td></tr></table>"##;

        assert_eq!(
            clean_table_html(input).unwrap(),
            concat!(
                "<table>\n",
                "  <tbody>\n",
                "    <tr>\n",
                "      <td><a>FFO details</a></td>\n",
                "    </tr>\n",
                "  </tbody>\n",
                "</table>"
            )
        );
    }

    #[test]
    fn rejects_unknown_markup_instead_of_guessing() {
        let error =
            clean_table_html("<table data-source=\"x\"><tr><td>FFO</td></tr></table>").unwrap_err();

        assert!(error.to_string().contains("unsupported attribute"));
    }

    #[test]
    fn rejects_multiple_or_nested_tables() {
        let error = clean_table_html(
            "<table><tr><td><table><tr><td>FFO</td></tr></table></td></tr></table>",
        )
        .unwrap_err();

        assert!(error.to_string().contains("multiple or nested tables"));
    }
}

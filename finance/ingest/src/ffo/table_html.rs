use anyhow::{Context, Result, bail};
use ego_tree::{NodeId, NodeRef};
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

    remove_visually_empty_rows(&mut document);
    attach_accounting_tokens(&mut document);
    remove_fully_empty_columns(&mut document);
    let before = semantic_projection(&document)?;

    normalize_nonbreaking_spaces(&mut document);
    remove_non_data_attributes(&mut document);
    unwrap_fonts(&mut document);
    unwrap_redundant_cell_wrappers(&mut document);
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

fn unwrap_redundant_cell_wrappers(document: &mut Html) {
    let wrapper_ids = document
        .tree
        .nodes()
        .filter(|node| is_redundant_cell_wrapper(*node))
        .map(|node| node.id())
        .collect::<Vec<_>>();

    for wrapper_id in wrapper_ids {
        let child_ids = document
            .tree
            .get(wrapper_id)
            .expect("cell wrapper remains in tree")
            .children()
            .map(|child| child.id())
            .collect::<Vec<_>>();
        for child_id in child_ids {
            document
                .tree
                .get_mut(wrapper_id)
                .expect("cell wrapper remains in tree")
                .insert_id_before(child_id);
        }
        document
            .tree
            .get_mut(wrapper_id)
            .expect("cell wrapper remains in tree")
            .detach();
    }
}

fn is_redundant_cell_wrapper(node: NodeRef<'_, Node>) -> bool {
    node.value()
        .as_element()
        .is_some_and(|element| matches!(element.name(), "div" | "p"))
        && node.parent().is_some_and(|parent| {
            parent
                .value()
                .as_element()
                .is_some_and(|element| matches!(element.name(), "td" | "th"))
                && parent.children().count() == 1
        })
}

fn remove_visually_empty_rows(document: &mut Html) {
    if document.tree.nodes().any(|node| {
        node.value().as_element().is_some_and(|element| {
            matches!(element.name(), "td" | "th") && element.attr("rowspan").is_some()
        })
    }) {
        return;
    }

    let row_ids = document
        .tree
        .nodes()
        .filter(|node| is_visually_empty_row(*node))
        .map(|node| node.id())
        .collect::<Vec<_>>();

    for row_id in row_ids {
        document
            .tree
            .get_mut(row_id)
            .expect("empty row remains in tree")
            .detach();
    }
}

#[derive(Clone, Copy)]
enum AccountingAttachment {
    PrefixDollar,
    SuffixClosingParenthesis,
}

struct AccountingTokenMove {
    source: NodeId,
    target: NodeId,
    attachment: AccountingAttachment,
}

fn attach_accounting_tokens(document: &mut Html) {
    let row_selector = Selector::parse("tr").expect("valid row selector");
    let moves = only_table(document)
        .expect("validated HTML has one table")
        .select(&row_selector)
        .flat_map(|row| {
            let cells = row
                .children()
                .filter_map(ElementRef::wrap)
                .filter(|cell| matches!(cell.value().name(), "td" | "th"))
                .collect::<Vec<_>>();
            let texts = cells
                .iter()
                .map(|cell| normalized_visible_text(*cell))
                .collect::<Vec<_>>();
            let mut moves = Vec::new();

            for (index, text) in texts.iter().enumerate() {
                let (attachment, target) = match text.as_str() {
                    "$" => (
                        AccountingAttachment::PrefixDollar,
                        ((index + 1)..cells.len()).find(|next| !texts[*next].is_empty()),
                    ),
                    ")" => (
                        AccountingAttachment::SuffixClosingParenthesis,
                        (0..index)
                            .rev()
                            .find(|previous| !texts[*previous].is_empty()),
                    ),
                    _ => continue,
                };
                let Some(target) = target else {
                    continue;
                };
                moves.push(AccountingTokenMove {
                    source: cells[index].id(),
                    target: cells[target].id(),
                    attachment,
                });
            }
            moves
        })
        .collect::<Vec<_>>();

    for token_move in moves {
        let text_id = match token_move.attachment {
            AccountingAttachment::PrefixDollar => {
                first_meaningful_text_id(document, token_move.target)
            }
            AccountingAttachment::SuffixClosingParenthesis => {
                last_meaningful_text_id(document, token_move.target)
            }
        };
        let Some(text_id) = text_id else {
            continue;
        };

        let mut text_node = document
            .tree
            .get_mut(text_id)
            .expect("target text remains in tree");
        let Node::Text(text) = text_node.value() else {
            unreachable!("accounting token targets are text nodes");
        };
        text.text = match token_move.attachment {
            AccountingAttachment::PrefixDollar => format!("${}", text.trim_start()).into(),
            AccountingAttachment::SuffixClosingParenthesis => {
                format!("{})", text.trim_end()).into()
            }
        };

        let child_ids = document
            .tree
            .get(token_move.source)
            .expect("accounting token source remains in tree")
            .children()
            .map(|child| child.id())
            .collect::<Vec<_>>();
        for child_id in child_ids {
            document
                .tree
                .get_mut(child_id)
                .expect("accounting token source child remains in tree")
                .detach();
        }
    }
}

fn normalized_visible_text(cell: ElementRef<'_>) -> String {
    cell.text().collect::<String>().trim().to_owned()
}

fn first_meaningful_text_id(document: &Html, cell_id: NodeId) -> Option<NodeId> {
    document.tree.get(cell_id)?.descendants().find_map(|node| {
        node.value()
            .as_text()
            .filter(|text| !text.trim().is_empty())
            .map(|_| node.id())
    })
}

fn last_meaningful_text_id(document: &Html, cell_id: NodeId) -> Option<NodeId> {
    document
        .tree
        .get(cell_id)?
        .descendants()
        .filter_map(|node| {
            node.value()
                .as_text()
                .filter(|text| !text.trim().is_empty())
                .map(|_| node.id())
        })
        .last()
}

#[derive(Debug)]
struct GridCell {
    id: NodeId,
    start: usize,
    colspan: usize,
    has_content: bool,
}

fn remove_fully_empty_columns(document: &mut Html) {
    // Column descriptors need corresponding contraction logic of their own.
    // Leave those comparatively rare tables unchanged for now.
    if document.tree.nodes().any(|node| {
        node.value()
            .as_element()
            .is_some_and(|element| matches!(element.name(), "col" | "colgroup"))
    }) {
        return;
    }

    while let Some((width, cells)) = resolve_grid(document) {
        let mut has_empty_single_cell = vec![false; width];
        let mut has_nonempty_single_cell = vec![false; width];
        for cell in &cells {
            if cell.colspan != 1 {
                continue;
            }
            if cell.has_content {
                has_nonempty_single_cell[cell.start] = true;
            } else {
                has_empty_single_cell[cell.start] = true;
            }
        }

        let mut removable = (0..width)
            .map(|column| has_empty_single_cell[column] && !has_nonempty_single_cell[column])
            .collect::<Vec<_>>();

        // A nonempty spanning cell must retain at least one coordinate. If its
        // entire range is otherwise removable, the range is structurally
        // ambiguous rather than provably empty, so preserve all of it.
        for cell in &cells {
            if cell.has_content
                && (cell.start..cell.start + cell.colspan).all(|column| removable[column])
            {
                removable[cell.start..cell.start + cell.colspan].fill(false);
            }
        }

        if !removable.iter().any(|remove| *remove) {
            break;
        }

        for cell in cells {
            let removed = removable[cell.start..cell.start + cell.colspan]
                .iter()
                .filter(|remove| **remove)
                .count();
            if removed == 0 {
                continue;
            }

            let new_colspan = cell.colspan - removed;
            if new_colspan == 0 {
                debug_assert!(!cell.has_content);
                document
                    .tree
                    .get_mut(cell.id)
                    .expect("empty cell remains in tree")
                    .detach();
            } else {
                set_colspan(document, cell.id, new_colspan);
            }
        }
    }
}

fn resolve_grid(document: &Html) -> Option<(usize, Vec<GridCell>)> {
    let table = only_table(document).ok()?;
    let row_selector = Selector::parse("tr").expect("valid row selector");
    let mut occupied_until = Vec::<usize>::new();
    let mut cells = Vec::new();
    let mut width = 0;
    let mut current_group = None;
    let mut row_index = 0;

    for row in table.select(&row_selector) {
        let group = row.ancestors().find_map(|ancestor| {
            ancestor.value().as_element().and_then(|element| {
                matches!(element.name(), "thead" | "tbody" | "tfoot").then(|| ancestor.id())
            })
        });
        if group != current_group {
            occupied_until.clear();
            current_group = group;
            row_index = 0;
        }

        let mut next_column = 0;
        for cell in row
            .children()
            .filter_map(ElementRef::wrap)
            .filter(|cell| matches!(cell.value().name(), "td" | "th"))
        {
            let colspan = parse_span(cell.value().attr("colspan"))?;
            let rowspan = parse_span(cell.value().attr("rowspan"))?;

            loop {
                while occupied_until
                    .get(next_column)
                    .is_some_and(|until| *until > row_index)
                {
                    next_column += 1;
                }
                let conflict = (next_column..next_column + colspan).find(|column| {
                    occupied_until
                        .get(*column)
                        .is_some_and(|until| *until > row_index)
                });
                if let Some(column) = conflict {
                    next_column = column + 1;
                } else {
                    break;
                }
            }

            let end = next_column + colspan;
            occupied_until.resize(occupied_until.len().max(end), 0);
            occupied_until[next_column..end].fill(row_index + rowspan);
            cells.push(GridCell {
                id: cell.id(),
                start: next_column,
                colspan,
                has_content: has_meaningful_content(cell),
            });
            width = width.max(end);
            next_column = end;
        }
        row_index += 1;
    }

    Some((width, cells))
}

fn parse_span(attribute: Option<&str>) -> Option<usize> {
    match attribute {
        None => Some(1),
        Some(value) => value.parse().ok().filter(|span| *span > 0),
    }
}

fn has_meaningful_content(cell: ElementRef<'_>) -> bool {
    cell.descendants()
        .any(|descendant| match descendant.value() {
            Node::Text(text) => !text.trim().is_empty(),
            Node::Comment(_) => true,
            Node::Element(element) => element.attrs().any(|(attribute, _)| {
                PRESERVED_ATTRIBUTES.contains(&attribute)
                    && !matches!(attribute, "colspan" | "rowspan")
            }),
            _ => false,
        })
}

fn set_colspan(document: &mut Html, cell_id: NodeId, colspan: usize) {
    let mut node = document
        .tree
        .get_mut(cell_id)
        .expect("contracted cell remains in tree");
    let Node::Element(element) = node.value() else {
        unreachable!("grid cells are elements");
    };

    if colspan == 1 {
        element
            .attrs
            .retain(|(attribute, _)| attribute.local.as_ref() != "colspan");
    } else {
        let (_, value) = element
            .attrs
            .iter_mut()
            .find(|(attribute, _)| attribute.local.as_ref() == "colspan")
            .expect("a contracted multi-column cell already has colspan");
        *value = colspan.to_string().into();
    }
}

fn is_visually_empty_row(node: NodeRef<'_, Node>) -> bool {
    node.value()
        .as_element()
        .is_some_and(|element| element.name() == "tr")
        && node
            .descendants()
            .all(|descendant| match descendant.value() {
                Node::Text(text) => text.trim().is_empty(),
                Node::Comment(_) => false,
                _ => true,
            })
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
        Node::Element(element) if element.name() == "font" || is_redundant_cell_wrapper(node) => {
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
    fn attaches_accounting_tokens_before_removing_their_columns() {
        let input = "<table><tr><td>$</td><td> (79,104</td><td>)</td><td><font>&nbsp;</font></td></tr></table>";

        assert_eq!(
            clean_table_html(input).unwrap(),
            concat!(
                "<table>\n",
                "  <tbody>\n",
                "    <tr>\n",
                "      <td>$(79,104)</td>\n",
                "    </tr>\n",
                "  </tbody>\n",
                "</table>"
            )
        );
    }

    #[test]
    fn attaches_across_empty_cells_without_crossing_rows() {
        let input = concat!(
            "<table>",
            "<tr><td>Amount</td><td>$</td><td></td><td><b>42</b></td></tr>",
            "<tr><td>Dangling tokens</td><td>)</td><td></td><td>$</td></tr>",
            "</table>"
        );

        assert_eq!(
            clean_table_html(input).unwrap(),
            concat!(
                "<table>\n",
                "  <tbody>\n",
                "    <tr>\n",
                "      <td>Amount</td>\n",
                "      <td><b>$42</b></td>\n",
                "    </tr>\n",
                "    <tr>\n",
                "      <td>Dangling tokens)</td>\n",
                "      <td>$</td>\n",
                "    </tr>\n",
                "  </tbody>\n",
                "</table>"
            )
        );
    }

    #[test]
    fn leaves_accounting_characters_embedded_in_text_unchanged() {
        let input = "<table><tr><td>US$</td><td>$ per share</td><td>42)</td></tr></table>";

        let cleaned = clean_table_html(input).unwrap();
        assert!(cleaned.contains("<td>US$</td>"));
        assert!(cleaned.contains("<td>$ per share</td>"));
        assert!(cleaned.contains("<td>42)</td>"));
    }

    #[test]
    fn accounting_token_columns_contract_crossing_spans() {
        let input = concat!(
            "<table>",
            "<tr><td></td><td colspan=\"3\">Year ended</td></tr>",
            "<tr><td>Net income</td><td>$</td><td>42</td><td></td></tr>",
            "<tr><td>Net loss</td><td colspan=\"2\">(7</td><td>)</td></tr>",
            "</table>"
        );

        assert_eq!(
            clean_table_html(input).unwrap(),
            concat!(
                "<table>\n",
                "  <tbody>\n",
                "    <tr>\n",
                "      <td></td>\n",
                "      <td>Year ended</td>\n",
                "    </tr>\n",
                "    <tr>\n",
                "      <td>Net income</td>\n",
                "      <td>$42</td>\n",
                "    </tr>\n",
                "    <tr>\n",
                "      <td>Net loss</td>\n",
                "      <td>(7)</td>\n",
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
    fn unwraps_a_cell_content_wrapper() {
        let input = "<table><tr><td><div>Net income <sup>(1)</sup></div></td><th><p>2025</p></th></tr></table>";

        assert_eq!(
            clean_table_html(input).unwrap(),
            concat!(
                "<table>\n",
                "  <tbody>\n",
                "    <tr>\n",
                "      <td>Net income <sup>(1)</sup></td>\n",
                "      <th>2025</th>\n",
                "    </tr>\n",
                "  </tbody>\n",
                "</table>"
            )
        );
    }

    #[test]
    fn preserves_multiple_cell_content_blocks() {
        let input = "<table><tr><td><div>Date of inception</div><div>through March 31</div></td></tr></table>";

        assert!(
            clean_table_html(input)
                .unwrap()
                .contains("<td><div>Date of inception</div><div>through March 31</div></td>")
        );
    }

    #[test]
    fn removes_rows_without_visible_content() {
        let input = concat!(
            "<table>",
            "<tr><td colspan=\"2\"></td></tr>",
            "<tr><td><div>&nbsp;</div></td><td><br></td></tr>",
            "<tr><td>FFO</td><td><sup>(1)</sup></td></tr>",
            "</table>"
        );

        assert_eq!(
            clean_table_html(input).unwrap(),
            concat!(
                "<table>\n",
                "  <tbody>\n",
                "    <tr>\n",
                "      <td>FFO</td>\n",
                "      <td><sup>(1)</sup></td>\n",
                "    </tr>\n",
                "  </tbody>\n",
                "</table>"
            )
        );
    }

    #[test]
    fn preserves_a_row_containing_a_comment() {
        let input = "<table><tr><td><!-- source note --></td></tr></table>";

        assert!(
            clean_table_html(input)
                .unwrap()
                .contains("<td><!-- source note --></td>")
        );
    }

    #[test]
    fn removes_empty_columns_and_contracts_crossing_headers() {
        let input = concat!(
            "<table>",
            "<tr><td></td><td colspan=\"4\">Years ended</td></tr>",
            "<tr><td>FFO</td><td></td><td>2025</td><td><br></td><td>2024</td></tr>",
            "<tr><td>Amount</td><td></td><td>$42</td><td></td><td>$38</td></tr>",
            "</table>"
        );

        assert_eq!(
            clean_table_html(input).unwrap(),
            concat!(
                "<table>\n",
                "  <tbody>\n",
                "    <tr>\n",
                "      <td></td>\n",
                "      <td colspan=\"2\">Years ended</td>\n",
                "    </tr>\n",
                "    <tr>\n",
                "      <td>FFO</td>\n",
                "      <td>2025</td>\n",
                "      <td>2024</td>\n",
                "    </tr>\n",
                "    <tr>\n",
                "      <td>Amount</td>\n",
                "      <td>$42</td>\n",
                "      <td>$38</td>\n",
                "    </tr>\n",
                "  </tbody>\n",
                "</table>"
            )
        );
    }

    #[test]
    fn resolves_rowspans_before_removing_an_empty_column() {
        let input = concat!(
            "<table>",
            "<tr><td rowspan=\"2\">FFO</td><td></td><td>2025</td></tr>",
            "<tr><td></td><td>$42</td></tr>",
            "</table>"
        );

        assert_eq!(
            clean_table_html(input).unwrap(),
            concat!(
                "<table>\n",
                "  <tbody>\n",
                "    <tr>\n",
                "      <td rowspan=\"2\">FFO</td>\n",
                "      <td>2025</td>\n",
                "    </tr>\n",
                "    <tr>\n",
                "      <td>$42</td>\n",
                "    </tr>\n",
                "  </tbody>\n",
                "</table>"
            )
        );
    }

    #[test]
    fn rowspans_do_not_cross_table_section_boundaries() {
        let input = concat!(
            "<table>",
            "<thead><tr><td rowspan=\"2\">Heading</td><td></td><td>Period</td></tr></thead>",
            "<tbody><tr><td>FFO</td><td></td><td>$42</td></tr></tbody>",
            "</table>"
        );

        let cleaned = clean_table_html(input).unwrap();
        assert!(cleaned.contains("<td rowspan=\"2\">Heading</td>"));
        assert!(cleaned.contains("<tr>\n      <td>FFO</td>\n      <td>$42</td>\n    </tr>"));
    }

    #[test]
    fn preserves_empty_rows_when_the_table_uses_rowspans() {
        let input = "<table><tr><td rowspan=\"2\">FFO</td></tr><tr><td></td></tr></table>";

        // The empty cell is a removable column, but the row itself remains so
        // the rowspan still covers two rows.
        assert!(clean_table_html(input).unwrap().contains("<tr></tr>"));
    }

    #[test]
    fn preserves_columns_when_only_spanning_content_defines_their_shape() {
        let input = "<table><tr><td colspan=\"2\">FFO reconciliation</td><td>Period</td></tr><tr><td></td><td></td><td>2025</td></tr></table>";

        let cleaned = clean_table_html(input).unwrap();
        assert!(cleaned.contains("<td colspan=\"2\">FFO reconciliation</td>"));
        assert_eq!(cleaned.matches("<td></td>").count(), 2);
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

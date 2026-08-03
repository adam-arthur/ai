use anyhow::{Context, Result, bail};
use ego_tree::{NodeId, NodeRef};
use scraper::{ElementRef, Html, Node, Selector, node::Text};

const ALLOWED_ELEMENTS: &[&str] = &[
    "a", "b", "br", "caption", "col", "colgroup", "div", "em", "font", "h1", "h2", "h3", "h4",
    "h5", "h6", "i", "p", "s", "small", "img", "span", "strong", "sub", "sup", "table", "tbody",
    "td", "tfoot", "th", "thead", "tr", "u",
];

const PRESERVED_ATTRIBUTES: &[&str] = &[
    "abbr", "colspan", "dir", "headers", "id", "lang", "rowspan", "scope", "title",
];

const STRUCTURAL_ELEMENTS: &[&str] = &["table", "colgroup", "thead", "tbody", "tfoot", "tr"];
const TEXT_TABLE_MAX_WIDTH: usize = 120;
const TEXT_TABLE_GUTTER: usize = 2;
const TEXT_TABLE_NUMERIC_MAX_WIDTH: usize = 24;

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

pub(super) fn render_table_text(table_html: &str) -> Result<String> {
    let cleaned = clean_table_html(table_html)?;
    let document = Html::parse_fragment(&cleaned);
    let grid = TextGrid::from_document(&document)?;
    Ok(grid.render(TEXT_TABLE_MAX_WIDTH))
}

#[derive(Debug)]
struct TextGridCell {
    start: usize,
    colspan: usize,
    text: String,
    header: bool,
}

#[derive(Debug)]
struct TextGridRow {
    cells: Vec<TextGridCell>,
}

#[derive(Debug)]
struct TextGrid {
    width: usize,
    rows: Vec<TextGridRow>,
}

impl TextGrid {
    fn from_document(document: &Html) -> Result<Self> {
        let table = only_table(document)?;
        let row_selector = Selector::parse("tr").expect("valid row selector");
        let mut occupied_until = Vec::<usize>::new();
        let mut rows = Vec::new();
        let mut width = 0;
        let mut current_group = None;
        let mut group_row_index = 0;

        for row in table.select(&row_selector) {
            let group = row.ancestors().find_map(|ancestor| {
                ancestor.value().as_element().and_then(|element| {
                    matches!(element.name(), "thead" | "tbody" | "tfoot").then(|| ancestor.id())
                })
            });
            if group != current_group {
                occupied_until.clear();
                current_group = group;
                group_row_index = 0;
            }

            let mut next_column = 0;
            let mut grid_cells = Vec::new();
            for cell in row
                .children()
                .filter_map(ElementRef::wrap)
                .filter(|cell| matches!(cell.value().name(), "td" | "th"))
            {
                let colspan = parse_span(cell.value().attr("colspan"))
                    .context("table cell has an invalid colspan")?;
                let rowspan = parse_span(cell.value().attr("rowspan"))
                    .context("table cell has an invalid rowspan")?;

                loop {
                    while occupied_until
                        .get(next_column)
                        .is_some_and(|until| *until > group_row_index)
                    {
                        next_column += 1;
                    }
                    let conflict = (next_column..next_column + colspan).find(|column| {
                        occupied_until
                            .get(*column)
                            .is_some_and(|until| *until > group_row_index)
                    });
                    if let Some(column) = conflict {
                        next_column = column + 1;
                    } else {
                        break;
                    }
                }

                let end = next_column + colspan;
                occupied_until.resize(occupied_until.len().max(end), 0);
                occupied_until[next_column..end].fill(group_row_index + rowspan);
                grid_cells.push(TextGridCell {
                    start: next_column,
                    colspan,
                    text: display_cell_text(cell),
                    header: cell.value().name() == "th",
                });
                width = width.max(end);
                next_column = end;
            }
            rows.push(TextGridRow { cells: grid_cells });
            group_row_index += 1;
        }

        Ok(Self { width, rows })
    }

    fn render(&self, max_width: usize) -> String {
        if self.width == 0 || self.rows.is_empty() {
            return String::new();
        }

        let header_rows = self.header_row_count();
        let numeric_columns = self.numeric_columns(header_rows);
        let widths = self.column_widths(max_width, &numeric_columns);
        let mut output = Vec::new();

        for (row_index, row) in self.rows.iter().enumerate() {
            output.extend(render_text_row(
                row,
                &widths,
                &numeric_columns,
                row_index < header_rows,
            ));
            if header_rows > 0 && row_index + 1 == header_rows {
                output.push(
                    widths
                        .iter()
                        .map(|width| "-".repeat(*width))
                        .collect::<Vec<_>>()
                        .join(&" ".repeat(TEXT_TABLE_GUTTER)),
                );
            }
        }

        let mut rendered = output.join("\n");
        rendered.push('\n');
        rendered
    }

    fn header_row_count(&self) -> usize {
        self.rows
            .iter()
            .take_while(|row| {
                let has_header_cell = row.cells.iter().any(|cell| cell.header);
                let first_column_is_empty = row
                    .cells
                    .iter()
                    .find(|cell| cell.start == 0)
                    .is_none_or(|cell| cell.text.is_empty());
                let is_full_width_heading = self.width > 1
                    && row.cells.len() == 1
                    && row.cells[0].start == 0
                    && row.cells[0].colspan == self.width;
                has_header_cell || first_column_is_empty || is_full_width_heading
            })
            .count()
    }

    fn numeric_columns(&self, header_rows: usize) -> Vec<bool> {
        (0..self.width)
            .map(|column| {
                let values = self
                    .rows
                    .iter()
                    .skip(header_rows)
                    .flat_map(|row| &row.cells)
                    .filter(|cell| cell.colspan == 1 && cell.start == column)
                    .map(|cell| cell.text.as_str())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>();
                !values.is_empty() && values.iter().all(|value| is_numeric_text(value))
            })
            .collect()
    }

    fn column_widths(&self, max_width: usize, numeric_columns: &[bool]) -> Vec<usize> {
        let mut preferred = vec![1; self.width];
        let mut minimum = vec![1; self.width];

        for cell in self.rows.iter().flat_map(|row| &row.cells) {
            if cell.colspan != 1 {
                continue;
            }
            let length = text_width(&cell.text).max(1);
            preferred[cell.start] = preferred[cell.start].max(length);
            let minimum_width = if numeric_columns[cell.start] && is_numeric_text(&cell.text) {
                length
            } else {
                longest_word_width(&cell.text).max(1)
            };
            minimum[cell.start] = minimum[cell.start].max(minimum_width);
        }

        for cell in self.rows.iter().flat_map(|row| &row.cells) {
            if cell.colspan == 1 || cell.text.is_empty() {
                continue;
            }
            let available = preferred[cell.start..cell.start + cell.colspan]
                .iter()
                .sum::<usize>()
                + TEXT_TABLE_GUTTER * (cell.colspan - 1);
            let needed = text_width(&cell.text);
            let mut deficit = needed.saturating_sub(available);
            let mut column = cell.start;
            while deficit > 0 {
                preferred[column] += 1;
                deficit -= 1;
                column += 1;
                if column == cell.start + cell.colspan {
                    column = cell.start;
                }
            }
        }

        for column in 0..self.width {
            if numeric_columns[column] {
                preferred[column] = preferred[column]
                    .min(TEXT_TABLE_NUMERIC_MAX_WIDTH)
                    .max(minimum[column]);
            }
        }

        let gutters = TEXT_TABLE_GUTTER * self.width.saturating_sub(1);
        while preferred.iter().sum::<usize>() + gutters > max_width {
            let Some(column) = (0..self.width)
                .filter(|column| preferred[*column] > minimum[*column])
                .max_by_key(|column| preferred[*column] - minimum[*column])
            else {
                break;
            };
            preferred[column] -= 1;
        }
        preferred
    }
}

fn display_cell_text(cell: ElementRef<'_>) -> String {
    let mut text = String::new();
    let mut pending_space = false;
    append_visible_text(cell, &mut text, &mut pending_space);
    let text = text.trim().to_owned();
    if text == "$—" {
        "—".to_owned()
    } else {
        text
    }
}

fn append_visible_text(node: ElementRef<'_>, output: &mut String, pending_space: &mut bool) {
    for child in node.children() {
        match child.value() {
            Node::Text(text) => {
                let raw = text.text.as_ref();
                let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
                if normalized.is_empty() {
                    if raw.chars().any(char::is_whitespace) {
                        *pending_space = !output.is_empty();
                    }
                    continue;
                }
                if *pending_space && !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(&normalized);
                *pending_space = raw.chars().last().is_some_and(char::is_whitespace);
            }
            Node::Element(element) if element.name() == "br" => {
                *pending_space = !output.is_empty();
            }
            Node::Element(element) => {
                let block = matches!(
                    element.name(),
                    "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                );
                if block {
                    *pending_space = !output.is_empty();
                }
                if let Some(child) = ElementRef::wrap(child) {
                    append_visible_text(child, output, pending_space);
                }
                if block {
                    *pending_space = !output.is_empty();
                }
            }
            Node::Document | Node::Fragment => {
                if let Some(child) = ElementRef::wrap(child) {
                    append_visible_text(child, output, pending_space);
                }
            }
            Node::Comment(_) | Node::Doctype(_) | Node::ProcessingInstruction(_) => {}
        }
    }
}

fn render_text_row(
    row: &TextGridRow,
    widths: &[usize],
    numeric_columns: &[bool],
    header_row: bool,
) -> Vec<String> {
    let mut rendered_cells = Vec::<(usize, Vec<String>, bool)>::new();
    let mut cursor = 0;
    for cell in &row.cells {
        while cursor < cell.start {
            rendered_cells.push((widths[cursor], vec![String::new()], false));
            cursor += 1;
        }
        let width = widths[cell.start..cell.start + cell.colspan]
            .iter()
            .sum::<usize>()
            + TEXT_TABLE_GUTTER * cell.colspan.saturating_sub(1);
        let right_aligned = cell.colspan == 1 && numeric_columns[cell.start] && !header_row;
        let continuation_indent = usize::from(cell.start == 0 && !header_row && width > 2) * 2;
        rendered_cells.push((
            width,
            wrap_text(&cell.text, width, continuation_indent),
            right_aligned,
        ));
        cursor = cell.start + cell.colspan;
    }
    while cursor < widths.len() {
        rendered_cells.push((widths[cursor], vec![String::new()], false));
        cursor += 1;
    }

    let height = rendered_cells
        .iter()
        .map(|(_, lines, _)| lines.len())
        .max()
        .unwrap_or(1);
    (0..height)
        .map(|line_index| {
            rendered_cells
                .iter()
                .map(|(width, lines, right_aligned)| {
                    let line = lines.get(line_index).map_or("", String::as_str);
                    pad_text(line, *width, *right_aligned)
                })
                .collect::<Vec<_>>()
                .join(&" ".repeat(TEXT_TABLE_GUTTER))
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn wrap_text(text: &str, width: usize, continuation_indent: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let separator = usize::from(!line.is_empty());
        let line_width = if lines.is_empty() {
            width
        } else {
            width - continuation_indent
        };
        if !line.is_empty() && text_width(&line) + separator + text_width(word) > line_width {
            lines.push(line);
            line = String::new();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    for line in lines.iter_mut().skip(1) {
        line.insert_str(0, &" ".repeat(continuation_indent));
    }
    lines
}

fn pad_text(text: &str, width: usize, right_aligned: bool) -> String {
    let padding = width.saturating_sub(text_width(text));
    if right_aligned {
        format!("{}{}", " ".repeat(padding), text)
    } else {
        format!("{}{}", text, " ".repeat(padding))
    }
}

fn text_width(text: &str) -> usize {
    text.chars().count()
}

fn longest_word_width(text: &str) -> usize {
    text.split_whitespace().map(text_width).max().unwrap_or(0)
}

fn is_numeric_text(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && trimmed.chars().all(|character| {
            character.is_ascii_digit()
                || matches!(
                    character,
                    '$' | ',' | '.' | '(' | ')' | '%' | '+' | '-' | '—' | '–' | ' ' | '\u{a0}'
                )
        })
}

fn clean_once(table_html: &str) -> Result<String> {
    let mut document = Html::parse_fragment(table_html);
    validate(&document)?;

    replace_images_with_text(&mut document);
    remove_visually_empty_rows(&mut document);
    attach_accounting_tokens(&mut document);
    remove_fully_empty_columns(&mut document);
    unwrap_fonts(&mut document);
    let before = semantic_projection(&document)?;

    normalize_nonbreaking_spaces(&mut document);
    remove_nonsemantic_attributes(&mut document);
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

fn replace_images_with_text(document: &mut Html) {
    let image_selector = Selector::parse("img").expect("valid image selector");
    let replacements = document
        .select(&image_selector)
        .map(|image| {
            let replacement = image
                .value()
                .attr("alt")
                .filter(|alt| !alt.trim().is_empty())
                .unwrap_or("(image)")
                .to_owned();
            (image.id(), replacement)
        })
        .collect::<Vec<_>>();

    for (id, replacement) in replacements {
        let mut image = document.tree.get_mut(id).expect("image remains in tree");
        image.insert_before(Node::Text(Text {
            text: replacement.into(),
        }));
        image.detach();
    }
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
                            && parent.children().all(|child| {
                                child
                                    .value()
                                    .as_text()
                                    .is_some_and(|text| text.trim().is_empty())
                            })
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

fn remove_nonsemantic_attributes(document: &mut Html) {
    let element_ids = document
        .tree
        .nodes()
        .filter(|node| node.value().is_element())
        .map(|node| node.id())
        .collect::<Vec<_>>();

    for id in element_ids {
        let mut node = document.tree.get_mut(id).expect("node remains in tree");
        if let Node::Element(element) = node.value() {
            element
                .attrs
                .retain(|(name, _)| is_preserved_attribute(name.local.as_ref()));
        }
    }
}

fn is_preserved_attribute(attribute: &str) -> bool {
    // Table data and layout depend on spans; the remaining attributes carry
    // useful semantic metadata. Styling, browser behavior, and vendor-specific
    // attributes do not affect the text grid and are safe to discard.
    PRESERVED_ATTRIBUTES.contains(&attribute)
}

fn unwrap_fonts(document: &mut Html) {
    let font_selector = Selector::parse("font").expect("valid font selector");
    let font_ids = document
        .select(&font_selector)
        .map(|font| font.id())
        .collect::<Vec<_>>();

    // Innermost-first keeps nested font contents in their original order.
    for font_id in font_ids.into_iter().rev() {
        if document
            .tree
            .get(font_id)
            .is_none_or(|font| font.parent().is_none())
        {
            continue;
        }
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
            parent.children().all(|child| {
                child.id() == node.id()
                    || child
                        .value()
                        .as_text()
                        .is_some_and(|text| text.trim().is_empty())
            }) && parent.value().as_element().is_some_and(|element| {
                matches!(element.name(), "td" | "th")
                    || (matches!(element.name(), "div" | "p") && is_redundant_cell_wrapper(parent))
            })
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
                is_preserved_attribute(attribute) && !matches!(attribute, "colspan" | "rowspan")
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
                .filter(|(attribute, _)| is_preserved_attribute(attribute))
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
    fn ignores_detached_font_wrapped_accounting_tokens() {
        let input = "<table><tr><td><font>$</font></td><td><font> (79,104</font></td><td><font>)</font></td></tr></table>";

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
    fn renders_a_borderless_table_with_headers_and_aligned_numbers() {
        let input = concat!(
            "<table>",
            "<tr><td></td><td colspan=\"3\">Three Months Ended</td>",
            "<td colspan=\"2\"><div>Period from January 23, 2015</div><div>through</div></td></tr>",
            "<tr><td></td><td colspan=\"3\">March 31, 2016</td><td colspan=\"2\">March 31, 2015</td></tr>",
            "<tr><td>Net loss</td><td>$</td><td>(150,000</td><td>)</td><td>$</td><td>—</td></tr>",
            "<tr><td>Add:</td><td colspan=\"3\"></td><td colspan=\"2\"></td></tr>",
            "<tr><td>Depreciation and amortization</td><td colspan=\"2\">—</td><td><br></td><td colspan=\"2\">—</td></tr>",
            "</table>"
        );

        let rendered = render_table_text(input).unwrap();
        let lines = rendered.lines().collect::<Vec<_>>();
        let normalized = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
        let rule_index = lines
            .iter()
            .position(|line| {
                line.contains('-') && line.chars().all(|character| matches!(character, '-' | ' '))
            })
            .unwrap();
        let net_loss = lines
            .iter()
            .find(|line| line.starts_with("Net loss"))
            .unwrap();
        let depreciation = lines
            .iter()
            .find(|line| line.starts_with("Depreciation"))
            .unwrap();

        assert!(rendered.contains("Three Months Ended"));
        assert!(normalized.contains("Period from January 23, 2015 through"));
        assert!(rendered.contains("March 31, 2016"));
        assert!(rendered.contains("March 31, 2015"));
        assert!(rule_index > 0);
        assert_eq!(
            net_loss.split_whitespace().collect::<Vec<_>>(),
            ["Net", "loss", "$(150,000)", "—"]
        );
        assert!(lines.contains(&"Add:"));
        assert_eq!(
            depreciation.split_whitespace().collect::<Vec<_>>(),
            ["Depreciation", "and", "amortization", "—", "—"]
        );
        assert!(lines.iter().all(|line| !line.ends_with(' ')));
        assert!(
            lines
                .iter()
                .all(|line| text_width(line) <= TEXT_TABLE_MAX_WIDTH)
        );
    }

    #[test]
    fn renders_a_one_row_bullet_layout_as_a_table() {
        let input =
            "<table><tr><td><p>·</p></td><td><p>FFO was $11.6 million.</p></td></tr></table>";

        assert_eq!(
            render_table_text(input).unwrap(),
            "·  FFO was $11.6 million.\n"
        );
    }

    #[test]
    fn renders_heading_markup_inside_cells() {
        let input = concat!(
            "<table><tr>",
            "<td><h2><font>Funds From Operations (FFO)</font></h2></td>",
            "<td><h2>2025</h2><h2>Actual</h2></td>",
            "</tr></table>"
        );

        assert_eq!(
            render_table_text(input).unwrap(),
            "Funds From Operations (FFO)  2025 Actual\n"
        );
    }

    #[test]
    fn wraps_long_table_text_to_the_output_width() {
        let prose =
            "This is a deliberately long sentence with repeated financial disclosure words. "
                .repeat(3);
        let input = format!("<table><tr><td>•</td><td>{prose}</td></tr></table>");

        let rendered = render_table_text(&input).unwrap();

        assert!(rendered.lines().count() > 1);
        assert!(
            rendered
                .lines()
                .all(|line| text_width(line) <= TEXT_TABLE_MAX_WIDTH)
        );
        assert!(rendered.lines().skip(1).all(|line| line.starts_with("   ")));
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
    fn preserves_whitespace_between_unwrapped_text_nodes() {
        let input =
            "<table><tr><td><font>FFO</font><font> </font><font>(1)</font></td></tr></table>";

        assert!(
            clean_table_html(input)
                .unwrap()
                .contains("<td>FFO (1)</td>")
        );
    }

    #[test]
    fn unwraps_nested_cell_wrappers_despite_whitespace_siblings() {
        let input = "<table><tr><td> <div><div><p>FFO</p></div></div> </td></tr></table>";

        let cleaned = clean_table_html(input).unwrap();
        assert!(!cleaned.contains("<div>"));
        assert!(!cleaned.contains("<p>"));
        assert_eq!(render_table_text(input).unwrap(), "FFO\n");
    }

    #[test]
    fn unwraps_wrappers_that_become_redundant_after_fonts() {
        let input = r##"<table><tr><td><font id="anchor"></font><div><a href="#page">32</a></div></td></tr></table>"##;

        assert_eq!(
            clean_table_html(input).unwrap(),
            concat!(
                "<table>\n",
                "  <tbody>\n",
                "    <tr>\n",
                "      <td><a>32</a></td>\n",
                "    </tr>\n",
                "  </tbody>\n",
                "</table>"
            )
        );
    }

    #[test]
    fn replaces_images_with_alt_text_or_an_indicator() {
        let with_alt =
            r#"<table><tr><td>FFO</td><td><img src="chart.png" alt="FFO chart"></td></tr></table>"#;
        let without_alt = r#"<table><tr><td>FFO</td><td><img src="chart.png"></td></tr></table>"#;

        assert_eq!(render_table_text(with_alt).unwrap(), "FFO  FFO chart\n");
        assert_eq!(render_table_text(without_alt).unwrap(), "FFO  (image)\n");
    }

    #[test]
    fn removes_legacy_presentational_and_navigation_attributes() {
        let input = r##"<table><tr><td><font color="black">FFO</font><a name="page"></a></td></tr></table>"##;

        assert_eq!(render_table_text(input).unwrap(), "FFO\n");
    }

    #[test]
    fn removes_custom_data_attributes() {
        let input = r#"<table data-source="filing"><tr data-row="result"><td data-celltype="desc">FFO</td></tr></table>"#;

        let cleaned = clean_table_html(input).unwrap();
        assert!(!cleaned.contains("data-"));
        assert_eq!(render_table_text(input).unwrap(), "FFO\n");
    }

    #[test]
    fn removes_nonsemantic_attributes_without_enumerating_them() {
        let input = concat!(
            "<table bordercollapse=\"collapse\" vendor-layout=\"fixed\">",
            "<tr><td onclick=\"alert('x')\">FFO</td></tr>",
            "</table>"
        );

        let cleaned = clean_table_html(input).unwrap();
        assert!(!cleaned.contains("bordercollapse"));
        assert!(!cleaned.contains("vendor-layout"));
        assert!(!cleaned.contains("onclick"));
        assert_eq!(render_table_text(input).unwrap(), "FFO\n");
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

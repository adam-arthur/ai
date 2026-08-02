use std::{
    collections::BTreeMap, fs, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}
};

use anyhow::{Context, Result, bail};
use scraper::{Html, Selector};

use super::table_html::render_table_text;

const MIN_CANDIDATE_SCORE: i32 = 12;
static STAGING_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct Candidate {
    table_html: String,
}

pub(super) fn ensure_candidate_tables(
    quarter_dir: &Path,
    sources: Vec<(PathBuf, Option<String>, Vec<u8>)>,
) -> Result<usize> {
    generate_candidate_tables(quarter_dir, sources)
}

fn generate_candidate_tables(
    quarter_dir: &Path,
    sources: Vec<(PathBuf, Option<String>, Vec<u8>)>,
) -> Result<usize> {
    let table_output_dir = quarter_dir.join("tables");

    let table_staging_dir = staging_dir(&table_output_dir)?;
    fs::create_dir(&table_staging_dir)
        .with_context(|| format!("failed to create {}", table_staging_dir.display()))?;

    let result = (|| {
        let mut candidates_by_document = BTreeMap::<String, Vec<Candidate>>::new();
        for (source, content_type, source_bytes) in sources {
            if is_pdf(&source, content_type.as_deref(), &source_bytes) {
                continue;
            }
            let source_document = source
                .file_name()
                .context("source document has no file name")?
                .to_string_lossy()
                .into_owned();
            let candidates = find_candidates(&String::from_utf8_lossy(&source_bytes));
            candidates_by_document
                .entry(source_document)
                .or_default()
                .extend(candidates);
        }

        let table_count = candidates_by_document.values().map(Vec::len).sum();
        for (source_document, candidates) in candidates_by_document {
            let candidate_count = candidates.len();
            for (offset, candidate) in candidates.into_iter().enumerate() {
                let table_path = table_staging_dir.join(candidate_table_name(
                    &source_document,
                    offset + 1,
                    candidate_count,
                ));
                write_candidate_table(&table_path, &candidate.table_html)?;
            }
        }

        publish_staging_dir(&table_staging_dir, &table_output_dir)?;
        Ok(table_count)
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&table_staging_dir);
    }
    result
}

fn candidate_table_name(source_document: &str, index: usize, candidate_count: usize) -> String {
    if candidate_count == 1 {
        format!("{source_document}.txt")
    } else {
        format!("{source_document}-{index}.txt")
    }
}

fn write_candidate_table(path: &Path, table_html: &str) -> Result<()> {
    let rendered = render_table_text(table_html)
        .with_context(|| format!("failed to render table for {}", path.display()))?;
    fs::write(path, rendered).with_context(|| format!("failed to write {}", path.display()))
}

fn publish_staging_dir(staging_dir: &Path, output_dir: &Path) -> Result<()> {
    if output_dir.exists() {
        let entries = fs::read_dir(staging_dir)
            .with_context(|| format!("failed to read {}", staging_dir.display()))?
            .collect::<std::io::Result<Vec<_>>>()?;
        for entry in &entries {
            let destination = output_dir.join(entry.file_name());
            if destination.exists() {
                bail!(
                    "FFO candidate artifact already exists: {}",
                    destination.display()
                );
            }
        }
        for entry in entries {
            let destination = output_dir.join(entry.file_name());
            fs::rename(entry.path(), &destination)
                .with_context(|| format!("failed to write {}", destination.display()))?;
        }
        fs::remove_dir(staging_dir)
            .with_context(|| format!("failed to remove {}", staging_dir.display()))?;
    } else {
        fs::rename(staging_dir, output_dir)
            .with_context(|| format!("failed to replace {}", output_dir.display()))?;
    }
    Ok(())
}

fn is_pdf(source: &Path, content_type: Option<&str>, bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF-")
        || content_type.is_some_and(|value| value.to_ascii_lowercase().contains("pdf"))
        || source
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

fn staging_dir(output_dir: &Path) -> Result<PathBuf> {
    let parent = output_dir
        .parent()
        .context("FFO table output has no parent directory")?;
    let name = output_dir
        .file_name()
        .context("FFO table output has no file name")?
        .to_string_lossy();
    let id = STAGING_ID.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(".{name}.tmp-{}-{id}", std::process::id())))
}

fn find_candidates(source: &str) -> Vec<Candidate> {
    let document = Html::parse_document(source);
    let table_selector = Selector::parse("table").expect("valid table selector");
    document
        .select(&table_selector)
        .filter(|table| score_candidate(&normalized_text(table.text())))
        .map(|table| Candidate {
            table_html: table.html(),
        })
        .collect()
}

fn score_candidate(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if ["guidance", "outlook", "forecast"]
        .iter()
        .any(|phrase| lower.contains(phrase))
        || (lower.contains("unchanged")
            && !["reconciliation", "net income", "net loss"]
                .iter()
                .any(|phrase| lower.contains(phrase)))
        || (lower.contains("full year")
            && lower.contains("growth")
            && !lower.contains("year ended"))
    {
        return false;
    }
    if !contains_word(&lower, "affo")
        && !contains_word(&lower, "ffo")
        && !lower.contains("funds from operations")
    {
        return false;
    }

    let numeric_tokens = text
        .split_whitespace()
        .filter(|token| token.chars().any(|character| character.is_ascii_digit()))
        .count();
    if numeric_tokens < 3 {
        return false;
    }

    let mut score = 8 + numeric_tokens.min(10) as i32;
    score += phrase_score(&lower, &["reconciliation"], 8);
    score += phrase_score(&lower, &["net income", "net loss"], 6);
    score += phrase_score(
        &lower,
        &[
            "three months ended",
            "six months ended",
            "nine months ended",
            "year ended",
            "years ended",
        ],
        5,
    );
    score += phrase_score(
        &lower,
        &["per diluted share", "per share", "diluted share"],
        4,
    );
    score += phrase_score(&lower, &["in thousands", "in millions"], 3);
    score -= phrase_score(&lower, &["definition", "we define"], 6);

    score >= MIN_CANDIDATE_SCORE
}

fn phrase_score(text: &str, phrases: &[&str], points: i32) -> i32 {
    if phrases.iter().any(|phrase| text.contains(phrase)) {
        points
    } else {
        0
    }
}

fn contains_word(text: &str, word: &str) -> bool {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .any(|token| token == word)
}

fn normalized_text<'a>(fragments: impl Iterator<Item = &'a str>) -> String {
    fragments
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_candidates_after_the_complete_source_file_name() {
        assert_eq!(
            candidate_table_name("q42025supplemental.htm", 2, 2),
            "q42025supplemental.htm-2.txt"
        );
        assert_eq!(
            candidate_table_name("q42025supplemental.htm", 1, 1),
            "q42025supplemental.htm.txt"
        );
    }

    #[test]
    fn writes_compact_candidate_table_text() {
        let test_dir = std::env::temp_dir().join(format!(
            "ffo-table-test-{}-{}",
            std::process::id(),
            STAGING_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&test_dir).unwrap();
        let table_path = test_dir.join("filing.htm-1.txt");
        let table_html = r#"<table style="width: 100%"><tr><td><font face="Arial">FFO</font></td><td align="right">(42)</td></tr></table>"#;

        write_candidate_table(&table_path, table_html).unwrap();

        assert_eq!(fs::read_to_string(&table_path).unwrap(), "FFO  (42)\n");
        fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn omits_singleton_suffix_and_numbers_colliding_document_names() {
        let test_dir = std::env::temp_dir().join(format!(
            "ffo-table-naming-test-{}-{}",
            std::process::id(),
            STAGING_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&test_dir).unwrap();
        let candidate_html = |label: &str| {
            format!(
                "<table><tr><td>Net income</td><td>1</td></tr><tr><td>{label} FFO reconciliation</td><td>2</td></tr><tr><td>Per share</td><td>3</td></tr></table>"
            )
            .into_bytes()
        };

        let sources = vec![
            (
                PathBuf::from("first/single.htm"),
                Some("text/html".to_owned()),
                candidate_html("Single"),
            ),
            (
                PathBuf::from("first/repeated.htm"),
                Some("text/html".to_owned()),
                candidate_html("First"),
            ),
            (
                PathBuf::from("second/repeated.htm"),
                Some("text/html".to_owned()),
                candidate_html("Second"),
            ),
        ];

        assert_eq!(generate_candidate_tables(&test_dir, sources).unwrap(), 3);
        let tables = test_dir.join("tables");
        assert!(tables.join("single.htm.txt").is_file());
        assert!(!tables.join("single.htm-1.txt").exists());
        assert!(tables.join("repeated.htm-1.txt").is_file());
        assert!(tables.join("repeated.htm-2.txt").is_file());

        fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn table_directory_accepts_additional_quarter_batches() {
        let test_dir = std::env::temp_dir().join(format!(
            "ffo-table-directory-test-{}-{}",
            std::process::id(),
            STAGING_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&test_dir).unwrap();

        assert_eq!(generate_candidate_tables(&test_dir, Vec::new()).unwrap(), 0);
        assert!(test_dir.join("tables").is_dir());
        assert!(!test_dir.join("vision").exists());
        assert_eq!(generate_candidate_tables(&test_dir, Vec::new()).unwrap(), 0);

        fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn finds_numeric_ffo_table() {
        let html = r#"
            <h2>Non-GAAP Reconciliation</h2>
            <p>Amounts in thousands, except per-share data</p>
            <table>
              <tr><th></th><th>Three Months Ended March 31, 2026</th></tr>
              <tr><td>Net income</td><td>24,011</td></tr>
              <tr><td>Real estate depreciation</td><td>66,993</td></tr>
              <tr><td>NAREIT FFO</td><td>90,354</td></tr>
              <tr><td>NAREIT FFO per diluted share</td><td>0.50</td></tr>
            </table>
        "#;

        let candidates = find_candidates(html);

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].table_html.contains("NAREIT FFO"));
    }

    #[test]
    fn rejects_definition_without_enough_numbers() {
        let html = r#"
            <table><tr><td>We define AFFO as adjusted funds from operations.</td></tr></table>
        "#;

        assert!(find_candidates(html).is_empty());
    }

    #[test]
    fn rejects_ffo_guidance_table() {
        let html = r#"
            <table>
              <tr><th>Full Year 2026 Guidance</th></tr>
              <tr><td>Core FFO per share</td><td>$1.70 - $1.76</td></tr>
              <tr><td>Core FFO growth</td><td>2.4% - 6.0%</td></tr>
            </table>
        "#;

        assert!(find_candidates(html).is_empty());
    }

    #[test]
    fn rejects_unnamed_guidance_range_table() {
        let html = r#"
            <table>
              <tr><th>Full Year 2026 (Unchanged)</th></tr>
              <tr><td>Core FFO per share</td><td>$1.70 - $1.76</td></tr>
              <tr><td>Core FFO growth</td><td>2.4% - 6.0%</td></tr>
            </table>
        "#;

        assert!(find_candidates(html).is_empty());
    }
}

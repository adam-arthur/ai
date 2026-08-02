use std::{
    collections::VecDeque, env, fs, path::{Path, PathBuf}, process::Command, sync::{
        Arc, atomic::{AtomicU64, Ordering}
    }
};

use anyhow::{Context, Result, bail};
use once_cell::sync::Lazy;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

const MAX_CONTEXT_ITEMS: usize = 4;
const MAX_CONTEXT_LENGTH: usize = 400;
const MIN_CANDIDATE_SCORE: i32 = 12;
const MAX_IMAGE_WIDTH: u32 = 2_400;
const MAX_IMAGE_HEIGHT: u32 = 16_000;
const MAX_CHROME_INSTANCES: usize = 4;

static CHROME_LIMIT: Lazy<Arc<Semaphore>> =
    Lazy::new(|| Arc::new(Semaphore::new(MAX_CHROME_INSTANCES)));
static CHROME: Lazy<std::result::Result<PathBuf, String>> =
    Lazy::new(|| resolve_chrome().map_err(|error| format!("{error:#}")));
static STAGING_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    source_document: String,
    candidates: Vec<ManifestCandidate>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestCandidate {
    index: usize,
    table_index: usize,
    image: String,
    score: i32,
    matched_terms: Vec<String>,
    context: Vec<String>,
    viewport_width: u32,
    viewport_height: u32,
}

#[derive(Debug)]
struct Candidate {
    table_index: usize,
    table_html: String,
    score: i32,
    matched_terms: Vec<String>,
    context: Vec<String>,
}

#[derive(Debug)]
struct CandidateScore {
    score: i32,
    matched_terms: Vec<String>,
}

pub(super) async fn ensure_candidate_images(
    source: &Path,
    content_type: Option<&str>,
    source_bytes: &[u8],
) -> Result<usize> {
    if is_pdf(source, content_type, source_bytes) {
        return Ok(0);
    }

    let source = source.to_path_buf();
    let source_bytes = source_bytes.to_vec();
    let permit = CHROME_LIMIT
        .clone()
        .acquire_owned()
        .await
        .context("FFO image-generation semaphore closed")?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        generate_candidate_images(&source, &source_bytes)
    })
    .await
    .context("FFO image-generation task panicked")?
}

fn generate_candidate_images(source: &Path, source_bytes: &[u8]) -> Result<usize> {
    let output_dir = output_dir(source)?;
    let manifest_path = output_dir.join("manifest.json");
    if manifest_path.is_file() {
        let manifest = read_complete_manifest(&manifest_path, &output_dir)?;
        return Ok(manifest.candidates.len());
    }
    if output_dir.exists() {
        bail!(
            "{} exists without a complete manifest; remove it before regenerating",
            output_dir.display()
        );
    }

    let html = String::from_utf8_lossy(source_bytes);
    let candidates = find_candidates(&html);
    let output_parent = output_dir
        .parent()
        .context("FFO image output has no parent directory")?;
    fs::create_dir_all(output_parent)
        .with_context(|| format!("failed to create {}", output_parent.display()))?;
    let staging_dir = staging_dir(&output_dir)?;
    fs::create_dir(&staging_dir)
        .with_context(|| format!("failed to create {}", staging_dir.display()))?;

    let source_document = source
        .file_name()
        .context("source document has no file name")?
        .to_string_lossy()
        .into_owned();
    let mut manifest = Manifest {
        source_document,
        candidates: Vec::with_capacity(candidates.len()),
    };

    let result = (|| {
        if !candidates.is_empty() {
            let chrome = chrome()?;
            for (offset, candidate) in candidates.into_iter().enumerate() {
                let index = offset + 1;
                let image_name = format!("candidate-{index:03}.png");
                let image_path = staging_dir.join(&image_name);
                let evidence_html = evidence_document(&candidate);
                let dimensions =
                    render_candidate(chrome, &staging_dir, index, &evidence_html, &image_path)?;
                manifest.candidates.push(ManifestCandidate {
                    index,
                    table_index: candidate.table_index,
                    image: image_name,
                    score: candidate.score,
                    matched_terms: candidate.matched_terms,
                    context: candidate.context,
                    viewport_width: dimensions.0,
                    viewport_height: dimensions.1,
                });
            }
        }

        write_manifest(&staging_dir.join("manifest.json"), &manifest)?;
        fs::rename(&staging_dir, &output_dir)
            .with_context(|| format!("failed to replace {}", output_dir.display()))?;
        Ok(manifest.candidates.len())
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&staging_dir);
    }
    result
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
        .context("FFO image output has no parent directory")?;
    let name = output_dir
        .file_name()
        .context("FFO image output has no file name")?
        .to_string_lossy();
    let id = STAGING_ID.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(".{name}.tmp-{}-{id}", std::process::id())))
}

fn output_dir(source: &Path) -> Result<PathBuf> {
    let parent = source
        .parent()
        .context("source document has no parent directory")?;
    let stem = source
        .file_stem()
        .context("source document has no file stem")?;
    Ok(parent.join("vision_ffo").join(stem))
}

fn read_complete_manifest(path: &Path, output_dir: &Path) -> Result<Manifest> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    for candidate in &manifest.candidates {
        let image = output_dir.join(&candidate.image);
        if !image.is_file() {
            bail!(
                "cached manifest references missing image {}; remove {} before regenerating",
                image.display(),
                output_dir.display()
            );
        }
    }
    Ok(manifest)
}

fn find_candidates(source: &str) -> Vec<Candidate> {
    let document = Html::parse_document(source);
    let content_selector =
        Selector::parse("h1, h2, h3, h4, h5, h6, p, table").expect("valid content selector");
    let mut context = VecDeque::<String>::new();
    let mut table_index = 0;
    let mut candidates = Vec::new();

    for element in document.select(&content_selector) {
        if element.value().name() != "table" {
            let text = normalized_text(element.text());
            if !text.is_empty() {
                let text = truncate_text(&text, MAX_CONTEXT_LENGTH);
                if context.back() != Some(&text) {
                    context.push_back(text);
                    while context.len() > MAX_CONTEXT_ITEMS {
                        context.pop_front();
                    }
                }
            }
            continue;
        }

        table_index += 1;
        let table_text = normalized_text(element.text());
        let Some(candidate_score) = score_candidate(&table_text) else {
            continue;
        };
        candidates.push(Candidate {
            table_index,
            table_html: element.html(),
            score: candidate_score.score,
            matched_terms: candidate_score.matched_terms,
            context: context.iter().cloned().collect(),
        });
    }

    candidates
}

fn score_candidate(text: &str) -> Option<CandidateScore> {
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
        return None;
    }
    let mut matched_terms = Vec::new();
    for (label, matches) in [
        ("AFFO", contains_word(&lower, "affo")),
        ("FFO", contains_word(&lower, "ffo")),
        (
            "funds from operations",
            lower.contains("funds from operations"),
        ),
    ] {
        if matches {
            matched_terms.push(label.to_owned());
        }
    }
    if matched_terms.is_empty() {
        return None;
    }

    let numeric_tokens = text
        .split_whitespace()
        .filter(|token| token.chars().any(|character| character.is_ascii_digit()))
        .count();
    if numeric_tokens < 3 {
        return None;
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

    (score >= MIN_CANDIDATE_SCORE).then_some(CandidateScore {
        score,
        matched_terms,
    })
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

fn truncate_text(text: &str, max_characters: usize) -> String {
    if text.chars().count() <= max_characters {
        return text.to_owned();
    }
    let mut truncated = text.chars().take(max_characters - 1).collect::<String>();
    truncated.push('…');
    truncated
}

fn evidence_document(candidate: &Candidate) -> String {
    let context = candidate
        .context
        .iter()
        .map(|text| format!("<div>{}</div>", escape_html(text)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'nonce-vision-ffo'; img-src data:">
<style>
html, body {{ margin: 0; padding: 0; background: white; }}
* {{ box-sizing: border-box; }}
#evidence {{ display: inline-block; padding: 28px; color: #111; background: white; font-family: Arial, Helvetica, sans-serif; }}
#context {{ max-width: 2200px; margin-bottom: 18px; font-size: 18px; line-height: 1.35; font-weight: 600; }}
#context div + div {{ margin-top: 5px; }}
#evidence table {{ width: auto !important; max-width: none !important; border-collapse: collapse !important; font-size: 18px !important; line-height: 1.25 !important; }}
#evidence th, #evidence td {{ min-width: 24px; max-width: 900px; padding: 5px 8px !important; vertical-align: middle; }}
#evidence font, #evidence span {{ font-size: inherit !important; line-height: inherit !important; }}
</style>
</head>
<body>
<main id="evidence">
<section id="context">{context}</section>
{table}
</main>
<script nonce="vision-ffo">
const evidence = document.getElementById('evidence');
let width = Math.ceil(evidence.getBoundingClientRect().width);
if (width > {max_width}) {{
  evidence.style.zoom = String({max_width} / width);
}}
const bounds = evidence.getBoundingClientRect();
document.documentElement.dataset.evidenceWidth = String(Math.ceil(bounds.width));
document.documentElement.dataset.evidenceHeight = String(Math.ceil(bounds.height));
</script>
</body>
</html>"#,
        context = context,
        table = candidate.table_html,
        max_width = MAX_IMAGE_WIDTH,
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_candidate(
    chrome: &Path,
    output_dir: &Path,
    index: usize,
    html: &str,
    image_path: &Path,
) -> Result<(u32, u32)> {
    let render_path = output_dir.join(format!(".candidate-{index:03}.html"));
    fs::write(&render_path, html)
        .with_context(|| format!("failed to write {}", render_path.display()))?;
    let file_url = reqwest::Url::from_file_path(&render_path)
        .map_err(|()| anyhow::anyhow!("failed to create file URL for {}", render_path.display()))?;

    let dumped = Command::new(chrome)
        .args(chrome_base_args())
        .arg("--dump-dom")
        .arg(file_url.as_str())
        .output()
        .with_context(|| format!("failed to run {}", chrome.display()))?;
    if !dumped.status.success() {
        let _ = fs::remove_file(&render_path);
        bail!(
            "Chrome failed while measuring candidate {index}: {}",
            String::from_utf8_lossy(&dumped.stderr).trim()
        );
    }
    let (width, height) = evidence_dimensions(&String::from_utf8_lossy(&dumped.stdout))?;
    if height > MAX_IMAGE_HEIGHT {
        let _ = fs::remove_file(&render_path);
        bail!(
            "candidate {index} is {height}px tall; tiling is not implemented yet (maximum {MAX_IMAGE_HEIGHT}px)"
        );
    }

    let screenshot = Command::new(chrome)
        .args(chrome_base_args())
        .arg(format!("--window-size={width},{height}"))
        .arg(format!("--screenshot={}", image_path.display()))
        .arg(file_url.as_str())
        .output()
        .with_context(|| format!("failed to run {}", chrome.display()))?;
    let _ = fs::remove_file(&render_path);
    if !screenshot.status.success() || !image_path.is_file() {
        bail!(
            "Chrome failed while rendering candidate {index}: {}",
            String::from_utf8_lossy(&screenshot.stderr).trim()
        );
    }
    Ok((width, height))
}

fn chrome_base_args() -> [&'static str; 6] {
    [
        "--headless=new",
        "--disable-background-networking",
        "--disable-gpu",
        "--hide-scrollbars",
        "--force-device-scale-factor=1",
        "--log-level=3",
    ]
}

fn evidence_dimensions(rendered_html: &str) -> Result<(u32, u32)> {
    let document = Html::parse_document(rendered_html);
    let selector = Selector::parse("html").expect("valid html selector");
    let root = document
        .select(&selector)
        .next()
        .context("Chrome output has no html element")?;
    let width = root
        .value()
        .attr("data-evidence-width")
        .context("Chrome output has no measured evidence width")?
        .parse::<u32>()
        .context("Chrome returned an invalid evidence width")?
        .clamp(1, MAX_IMAGE_WIDTH);
    let height = root
        .value()
        .attr("data-evidence-height")
        .context("Chrome output has no measured evidence height")?
        .parse::<u32>()
        .context("Chrome returned an invalid evidence height")?
        .max(1);
    Ok((width, height))
}

fn resolve_chrome() -> Result<PathBuf> {
    let candidates = env::var_os("CHROME_BIN")
        .map(PathBuf::from)
        .into_iter()
        .chain(
            [
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                "/Applications/Chromium.app/Contents/MacOS/Chromium",
                "/usr/bin/google-chrome",
                "/usr/bin/chromium",
                "/usr/bin/chromium-browser",
            ]
            .map(PathBuf::from),
        );
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!("Chrome/Chromium not found; set CHROME_BIN")
}

fn chrome() -> Result<&'static Path> {
    CHROME
        .as_ref()
        .map(PathBuf::as_path)
        .map_err(|error| anyhow::anyhow!(error.clone()))
}

fn write_manifest(path: &Path, manifest: &Manifest) -> Result<()> {
    let temporary = path.with_extension("json.tmp");
    let mut bytes = serde_json::to_vec_pretty(manifest)?;
    bytes.push(b'\n');
    fs::write(&temporary, bytes)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("failed to replace {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_numeric_ffo_table_and_retains_nearby_context() {
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
        assert_eq!(candidates[0].table_index, 1);
        assert!(candidates[0].matched_terms.contains(&"FFO".to_owned()));
        assert_eq!(
            candidates[0].context,
            [
                "Non-GAAP Reconciliation",
                "Amounts in thousands, except per-share data"
            ]
        );
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

    #[test]
    fn parses_dimensions_added_by_render_page() {
        let html = r#"<html data-evidence-width="1832" data-evidence-height="947"></html>"#;

        assert_eq!(evidence_dimensions(html).unwrap(), (1832, 947));
    }
}

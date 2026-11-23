// Content-shape audit for Coursera URLs in the canonical fixture.
//
// Run:
//   cargo run --release --bin audit-courses -- ../fixtures/expected-courses.md
//
// Exits non-zero if any URL fails the content-shape check (soft-404, empty
// title, or anchor-text/page-title mismatch).

use jugar_probar::{Browser, BrowserConfig};
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
struct CourseLink {
    url: String,
    expected_text: String,
}

#[derive(Debug, Serialize)]
struct AuditResult {
    url: String,
    expected_text: String,
    title: Option<String>,
    body_excerpt: Option<String>,
    status: String,
    reason: Option<String>,
}

const SOFT_404_MARKERS: &[&str] = &[
    "page you requested could not be found",
    "we can't find that page",
    "page not found",
    "course is no longer available",
    "course is unavailable",
    "this course is not available",
    "sorry, the page",
];

fn parse_fixture(path: &std::path::Path) -> std::io::Result<Vec<CourseLink>> {
    let content = std::fs::read_to_string(path)?;
    // Accept any http(s) URL inside a markdown link. Works for both the
    // Coursera fixture and the noahgift.com site fixture.
    let re = Regex::new(r"\[([^\]]+)\]\((https?://[^\s)]+)\)").unwrap();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for cap in re.captures_iter(&content) {
        let url = cap[2].to_string();
        if seen.insert(url.clone()) {
            out.push(CourseLink {
                url,
                expected_text: cap[1].to_string(),
            });
        }
    }
    Ok(out)
}

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn classify(
    link: &CourseLink,
    title: &Option<String>,
    body: &Option<String>,
) -> (String, Option<String>) {
    let title_s = title.as_deref().unwrap_or("");
    let body_s = body.as_deref().unwrap_or("");
    let combined_lc = format!("{} {}", title_s, body_s).to_lowercase();

    if let Some(marker) = SOFT_404_MARKERS.iter().find(|m| combined_lc.contains(*m)) {
        return (
            "soft-404".into(),
            Some(format!("dead-page marker: '{}'", marker)),
        );
    }

    if title_s.is_empty() {
        return (
            "empty-title".into(),
            Some("document.title was empty".into()),
        );
    }

    // Pass if expected_text matches the page title (Coursera-style course pages)
    // OR appears in the rendered body text (faculty / partner / book pages).
    // Two-way substring match handles both shorter README anchors ("Noah Gift"
    // inside "Noah Gift | Faculty") and longer anchors that contain the title.
    let title_clean = title_s
        .rsplit_once('|')
        .map_or(title_s, |(left, _)| left.trim());
    let want = normalize(&link.expected_text);
    let title_n = normalize(title_clean);
    let body_n = normalize(body_s);

    let title_match = title_n.contains(&want) || want.contains(&title_n);
    let body_match = body_n.contains(&want);

    if !title_match && !body_match {
        return (
            "content-mismatch".into(),
            Some(format!(
                "expected '{}' in title or body, title='{}'",
                link.expected_text, title_clean
            )),
        );
    }

    ("ok".into(), None)
}

async fn audit_one(browser: &Browser, link: &CourseLink) -> AuditResult {
    let mut page = match browser.new_page().await {
        Ok(p) => p,
        Err(e) => {
            return AuditResult {
                url: link.url.clone(),
                expected_text: link.expected_text.clone(),
                title: None,
                body_excerpt: None,
                status: "error".into(),
                reason: Some(format!("new_page: {e}")),
            };
        }
    };

    if let Err(e) = page.goto(&link.url).await {
        return AuditResult {
            url: link.url.clone(),
            expected_text: link.expected_text.clone(),
            title: None,
            body_excerpt: None,
            status: "error".into(),
            reason: Some(format!("goto: {e}")),
        };
    }

    // Coursera is a SPA; give the client app time to hydrate before asserting.
    tokio::time::sleep(Duration::from_secs(4)).await;

    let title: Option<String> = page
        .evaluate("document.title")
        .await
        .ok()
        .and_then(|r| r.into_value::<serde_json::Value>().ok())
        .and_then(|v| v.as_str().map(String::from));

    let body: Option<String> = page
        .evaluate("(document.body && document.body.innerText || '').slice(0, 4000)")
        .await
        .ok()
        .and_then(|r| r.into_value::<serde_json::Value>().ok())
        .and_then(|v| v.as_str().map(String::from));

    let (status, reason) = classify(link, &title, &body);

    AuditResult {
        url: link.url.clone(),
        expected_text: link.expected_text.clone(),
        title,
        body_excerpt: body,
        status,
        reason,
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fixture: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // Default: from tests/course-audit, fixture sits at ../fixtures/.
            PathBuf::from("../fixtures/expected-courses.md")
        });

    let links = parse_fixture(&fixture)?;
    eprintln!("Auditing {} URLs from {}", links.len(), fixture.display());

    let mut config = BrowserConfig::default();
    config.viewport_width = 1280;
    config.viewport_height = 800;
    config.sandbox = false; // common requirement on Linux CI
    config.user_agent = Some(
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
         Chrome/120.0.0.0 Safari/537.36"
            .into(),
    );

    let browser = Browser::launch(config).await?;
    let mut results = Vec::with_capacity(links.len());

    for (i, link) in links.iter().enumerate() {
        eprintln!("[{:3}/{}] {}", i + 1, links.len(), link.url);
        let r = audit_one(&browser, link).await;
        if r.status != "ok" {
            eprintln!(
                "    -> {} ({})",
                r.status,
                r.reason.as_deref().unwrap_or("")
            );
        }
        results.push(r);
    }

    let bad: Vec<&AuditResult> = results.iter().filter(|r| r.status != "ok").collect();
    eprintln!();
    eprintln!("=== Audit summary ===");
    eprintln!("Total:    {}", results.len());
    eprintln!("OK:       {}", results.len() - bad.len());
    eprintln!("Failures: {}", bad.len());

    let json = serde_json::to_string_pretty(&results)?;
    let report_name = fixture
        .file_stem()
        .map(|s| format!("{}-report.json", s.to_string_lossy()))
        .unwrap_or_else(|| "audit-report.json".into());
    std::fs::write(&report_name, &json)?;
    eprintln!("Wrote {}", report_name);

    if !bad.is_empty() {
        eprintln!();
        eprintln!("--- Failing URLs ---");
        for r in &bad {
            eprintln!("[{}] {}", r.status, r.url);
            if let Some(reason) = &r.reason {
                eprintln!("    {}", reason);
            }
        }
    }

    let _ = browser.close().await;

    if bad.is_empty() {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

//! Recipe loading, frontmatter parsing, markdown rendering, and git operations.

use crate::auth::hex_encode;
use crate::models::{Ingredient, Recipe};
use chrono::{DateTime, TimeZone, Utc};
use pulldown_cmark::Parser;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use walkdir::WalkDir;

// ============================================================================
// Frontmatter Parsing
// ============================================================================

#[derive(Debug, Default)]
pub struct Frontmatter {
    pub title: Option<String>,
    pub servings: Option<u32>,
    pub tags: Vec<String>,
    pub ingredients: Vec<Ingredient>,
}

/// Parse frontmatter and body from recipe markdown content.
pub fn parse_frontmatter(content: &str) -> (Frontmatter, String) {
    let mut fm = Frontmatter::default();
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() || lines[0].trim() != "---" {
        return (fm, content.to_string());
    }

    let mut end_idx = None;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            end_idx = Some(i);
            break;
        }
    }

    let end_idx = match end_idx {
        Some(i) => i,
        None => return (fm, content.to_string()),
    };

    let mut in_ingredients = false;
    let mut current_ingredient: Option<(String, f64, String)> = None;

    for line in &lines[1..end_idx] {
        let trimmed = line.trim();

        // If we're in ingredients block and this is a continuation line
        if in_ingredients && (line.starts_with("    ") || line.starts_with("\t\t")) {
            if let Some((key, value)) = trimmed.split_once(':') {
                let key = key.trim().to_lowercase();
                let value = value.trim();
                match key.as_str() {
                    "name" => {
                        if let Some(ref mut ing) = current_ingredient {
                            ing.0 = value.to_string();
                        }
                    }
                    "qty" => {
                        if let Some(ref mut ing) = current_ingredient {
                            ing.1 = value.parse().unwrap_or(0.0);
                        }
                    }
                    "unit" => {
                        if let Some(ref mut ing) = current_ingredient {
                            ing.2 = value.to_string();
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }

        // Start of a new ingredient list item
        if in_ingredients && trimmed.starts_with("- name:") {
            // Flush previous ingredient
            if let Some((name, qty, unit)) = current_ingredient.take() {
                if !name.is_empty() {
                    fm.ingredients.push(Ingredient { name, qty, unit });
                }
            }
            let name = trimmed
                .strip_prefix("- name:")
                .unwrap_or("")
                .trim()
                .to_string();
            current_ingredient = Some((name, 0.0, String::new()));
            continue;
        }

        // Non-continuation, non-list-item line while in ingredients => end block
        if in_ingredients && !trimmed.is_empty() && !trimmed.starts_with('-') {
            // Flush last ingredient
            if let Some((name, qty, unit)) = current_ingredient.take() {
                if !name.is_empty() {
                    fm.ingredients.push(Ingredient { name, qty, unit });
                }
            }
            in_ingredients = false;
        }

        // Top-level key: value
        if !in_ingredients {
            if let Some((key, value)) = trimmed.split_once(':') {
                let key = key.trim().to_lowercase();
                let value = value.trim();

                match key.as_str() {
                    "title" => fm.title = Some(value.to_string()),
                    "servings" => fm.servings = value.parse().ok(),
                    "tags" => {
                        fm.tags = value
                            .split(',')
                            .map(|t| t.trim().to_string())
                            .filter(|t| !t.is_empty())
                            .collect();
                    }
                    "ingredients" => {
                        in_ingredients = true;
                    }
                    _ => {}
                }
            }
        }
    }

    // Flush final ingredient
    if let Some((name, qty, unit)) = current_ingredient.take() {
        if !name.is_empty() {
            fm.ingredients.push(Ingredient { name, qty, unit });
        }
    }

    let body = lines[end_idx + 1..]
        .join("\n")
        .trim_start_matches('\n')
        .to_string();

    (fm, body)
}

/// Serialize a recipe back to the frontmatter + body markdown format.
pub fn serialize_recipe(
    title: &str,
    servings: Option<u32>,
    tags: &[String],
    ingredients: &[Ingredient],
    body: &str,
) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("title: {}\n", title));
    if let Some(s) = servings {
        out.push_str(&format!("servings: {}\n", s));
    }
    if !tags.is_empty() {
        out.push_str(&format!("tags: {}\n", tags.join(", ")));
    }
    if !ingredients.is_empty() {
        out.push_str("ingredients:\n");
        for ing in ingredients {
            out.push_str(&format!("  - name: {}\n", ing.name));
            out.push_str(&format!("    qty: {}\n", ing.qty));
            out.push_str(&format!("    unit: {}\n", ing.unit));
        }
    }
    out.push_str("---\n\n");
    out.push_str(body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
    out
}

// ============================================================================
// Key Generation & HTML Helpers
// ============================================================================

/// Generate a unique key from a file path (first 6 hex chars of SHA-256).
pub fn generate_key(path: &PathBuf) -> String {
    let relative = path.to_string_lossy().replace('\\', "/");
    let mut hasher = Sha256::new();
    hasher.update(relative.as_bytes());
    let hash = hasher.finalize();
    hex_encode(&hash[..3])
}

/// Escape HTML special characters.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Escape for a JavaScript single-quoted string literal.
pub fn js_single_quote_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(ch),
        }
    }
    out
}

/// Escape for embedding a JS single-quoted string inside an HTML attribute.
pub fn js_single_quote_attr_escape(s: &str) -> String {
    html_escape(&js_single_quote_escape(s))
}

/// Render markdown body to sanitized HTML.
pub fn render_markdown(md: &str) -> String {
    let parser = Parser::new(md);
    let mut html_output = String::new();
    pulldown_cmark::html::push_html(&mut html_output, parser);
    ammonia::clean(&html_output)
}

// ============================================================================
// File I/O
// ============================================================================

/// Load a single recipe from a file path.
pub fn load_recipe(path: &PathBuf, content_dir: &PathBuf) -> Option<Recipe> {
    let content = fs::read_to_string(path).ok()?;
    let (fm, body) = parse_frontmatter(&content);
    let title = fm.title.unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    });
    let body_html = render_markdown(&body);
    let key = generate_key(&path.strip_prefix(content_dir).ok()?.to_path_buf());

    let metadata = fs::metadata(path).ok()?;
    let modified: DateTime<Utc> = metadata
        .modified()
        .ok()
        .and_then(|t| {
            let dur = t.duration_since(std::time::UNIX_EPOCH).ok()?;
            Utc.timestamp_opt(dur.as_secs() as i64, 0).single()
        })
        .unwrap_or_else(Utc::now);

    Some(Recipe {
        key,
        title,
        servings: fm.servings,
        tags: fm.tags,
        ingredients: fm.ingredients,
        body_markdown: body,
        body_html,
        path: path.clone(),
        modified,
    })
}

/// Load all recipes from the content directory.
pub fn load_all_recipes(content_dir: &PathBuf) -> Vec<Recipe> {
    let mut recipes = Vec::new();

    for entry in WalkDir::new(content_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path().to_path_buf();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Some(recipe) = load_recipe(&path, content_dir) {
                recipes.push(recipe);
            }
        }
    }

    recipes.sort_by(|a, b| b.modified.cmp(&a.modified));
    recipes
}

// ============================================================================
// Git Operations
// ============================================================================

/// Git add and commit a recipe file change.
pub fn git_commit(content_dir: &PathBuf, path: &PathBuf, message: &str) {
    let dir = content_dir.parent().unwrap_or(content_dir);

    // git add the file
    let _ = Command::new("git")
        .args(["add", &path.to_string_lossy()])
        .current_dir(dir)
        .output();

    // git commit
    let _ = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(dir)
        .output();
}

/// Git rm and commit a recipe file deletion.
pub fn git_rm_commit(content_dir: &PathBuf, path: &PathBuf, message: &str) {
    let dir = content_dir.parent().unwrap_or(content_dir);

    let _ = Command::new("git")
        .args(["rm", &path.to_string_lossy()])
        .current_dir(dir)
        .output();

    let _ = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(dir)
        .output();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter_valid() {
        let content = r#"---
title: Chicken Stir Fry
servings: 4
tags: dinner, asian
ingredients:
  - name: chicken breast
    qty: 500
    unit: g
  - name: soy sauce
    qty: 3
    unit: tbsp
---

## Instructions

1. Cut chicken into strips.
"#;
        let (fm, body) = parse_frontmatter(content);
        assert_eq!(fm.title.unwrap(), "Chicken Stir Fry");
        assert_eq!(fm.servings.unwrap(), 4);
        assert_eq!(fm.tags, vec!["dinner", "asian"]);
        assert_eq!(fm.ingredients.len(), 2);
        assert_eq!(fm.ingredients[0].name, "chicken breast");
        assert_eq!(fm.ingredients[0].qty, 500.0);
        assert_eq!(fm.ingredients[0].unit, "g");
        assert_eq!(fm.ingredients[1].name, "soy sauce");
        assert_eq!(fm.ingredients[1].qty, 3.0);
        assert_eq!(fm.ingredients[1].unit, "tbsp");
        assert!(body.contains("## Instructions"));
    }

    #[test]
    fn test_parse_frontmatter_empty() {
        let content = "Just some markdown content.";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.title.is_none());
        assert!(fm.ingredients.is_empty());
        assert_eq!(body, content);
    }

    #[test]
    fn test_parse_frontmatter_no_closing() {
        let content = "---\ntitle: Incomplete\nNo closing fence";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.title.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn test_parse_frontmatter_no_ingredients() {
        let content = "---\ntitle: Simple Recipe\nservings: 2\n---\n\nJust do it.\n";
        let (fm, body) = parse_frontmatter(content);
        assert_eq!(fm.title.unwrap(), "Simple Recipe");
        assert_eq!(fm.servings.unwrap(), 2);
        assert!(fm.ingredients.is_empty());
        assert!(body.contains("Just do it"));
    }

    #[test]
    fn test_serialize_roundtrip() {
        let ingredients = vec![
            Ingredient {
                name: "flour".into(),
                qty: 2.0,
                unit: "cups".into(),
            },
            Ingredient {
                name: "sugar".into(),
                qty: 0.5,
                unit: "cups".into(),
            },
        ];
        let tags = vec!["baking".to_string(), "dessert".to_string()];
        let body = "## Steps\n\n1. Mix ingredients.";

        let serialized = serialize_recipe("Cake", Some(8), &tags, &ingredients, body);
        let (fm, parsed_body) = parse_frontmatter(&serialized);

        assert_eq!(fm.title.unwrap(), "Cake");
        assert_eq!(fm.servings.unwrap(), 8);
        assert_eq!(fm.tags, tags);
        assert_eq!(fm.ingredients.len(), 2);
        assert_eq!(fm.ingredients[0].name, "flour");
        assert_eq!(fm.ingredients[0].qty, 2.0);
        assert!(parsed_body.contains("Mix ingredients"));
    }

    #[test]
    fn test_generate_key() {
        let path = PathBuf::from("chicken-stir-fry.md");
        let key = generate_key(&path);
        assert_eq!(key.len(), 6);
        // Deterministic
        assert_eq!(key, generate_key(&path));
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<b>bold</b>"), "&lt;b&gt;bold&lt;/b&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape(r#"say "hi""#), "say &quot;hi&quot;");
    }

    #[test]
    fn test_render_markdown() {
        let html = render_markdown("**bold** text");
        assert!(html.contains("<strong>bold</strong>"));
    }

    #[test]
    fn test_parse_fractional_qty() {
        let content =
            "---\ntitle: Tea\ningredients:\n  - name: honey\n    qty: 0.5\n    unit: tbsp\n---\n";
        let (fm, _) = parse_frontmatter(content);
        assert_eq!(fm.ingredients[0].qty, 0.5);
    }

    #[test]
    fn test_js_single_quote_escape() {
        let escaped = js_single_quote_escape("O'Reilly\\n");
        assert_eq!(escaped, "O\\'Reilly\\\\n");
    }

    #[test]
    fn test_js_single_quote_attr_escape() {
        let escaped = js_single_quote_attr_escape("x'\"<y");
        assert_eq!(escaped, "x\\'&quot;&lt;y");
    }

    #[test]
    fn test_generate_key_uses_path_not_only_filename() {
        let a = PathBuf::from("dir-a/recipe.md");
        let b = PathBuf::from("dir-b/recipe.md");
        assert_ne!(generate_key(&a), generate_key(&b));
    }
}

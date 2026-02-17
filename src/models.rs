//! Data structures for the recipes application.

use serde::{Deserialize, Serialize};

/// A single ingredient with quantity and unit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ingredient {
    pub name: String,
    pub qty: f64,
    pub unit: String,
}

/// A parsed recipe loaded from a markdown file.
#[derive(Debug, Clone)]
pub struct Recipe {
    pub key: String,
    pub title: String,
    pub servings: Option<u32>,
    pub tags: Vec<String>,
    pub ingredients: Vec<Ingredient>,
    pub body_markdown: String,
    pub body_html: String,
    pub path: std::path::PathBuf,
    pub modified: chrono::DateTime<chrono::Utc>,
}

/// A user's selection of a recipe with a multiplier.
#[derive(Debug, Clone, Deserialize)]
pub struct RecipeSelection {
    pub key: String,
    pub multiplier: f64,
}

/// An item on the shopping list (aggregated across recipes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShoppingItem {
    pub name: String,
    pub qty: f64,
    pub unit: String,
    pub in_pantry: bool,
    pub sources: Vec<String>,
}

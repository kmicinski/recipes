//! Recipes library - re-exports for testing and external use.

use sled::Db;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub mod auth;
pub mod handlers;
pub mod models;
pub mod pantry;
pub mod recipes;
pub mod shopping;
pub mod templates;

pub const CONTENT_DIR: &str = "content";
pub const DB_PATH: &str = ".recipes_db";

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub content_dir: PathBuf,
    pub db: Db,
}

impl AppState {
    pub fn new() -> Self {
        let content_dir = PathBuf::from(CONTENT_DIR);
        fs::create_dir_all(&content_dir).ok();

        let db = sled::open(DB_PATH).expect("Failed to open database");

        Self { content_dir, db }
    }

    pub fn load_recipes(&self) -> Vec<models::Recipe> {
        recipes::load_all_recipes(&self.content_dir)
    }

    pub fn recipes_map(&self) -> HashMap<String, models::Recipe> {
        self.load_recipes()
            .into_iter()
            .map(|r| (r.key.clone(), r))
            .collect()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate that a path stays within the given base directory.
pub fn validate_path_within(base: &PathBuf, target: &PathBuf) -> Result<PathBuf, String> {
    let canonical_base =
        fs::canonicalize(base).map_err(|e| format!("Cannot resolve base directory: {}", e))?;

    if target.exists() {
        let canonical =
            fs::canonicalize(target).map_err(|e| format!("Cannot resolve path: {}", e))?;
        if canonical.starts_with(&canonical_base) {
            Ok(canonical)
        } else {
            Err("Path escapes base directory".to_string())
        }
    } else {
        let parent = target.parent().ok_or("No parent directory")?;
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create directory: {}", e))?;
        let canonical_parent =
            fs::canonicalize(parent).map_err(|e| format!("Cannot resolve parent: {}", e))?;
        if canonical_parent.starts_with(&canonical_base) {
            Ok(target.clone())
        } else {
            Err("Path escapes base directory".to_string())
        }
    }
}

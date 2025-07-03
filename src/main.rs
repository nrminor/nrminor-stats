#![warn(clippy::pedantic)]

use anyhow::Result;
use std::env;

mod cache;
mod github_client;
mod stats;
mod svg_generator;

use crate::{stats::StatsCollector, svg_generator::SvgGenerator};

#[tokio::main]
async fn main() -> Result<()> {
    // Get environment variables
    let access_token = env::var("ACCESS_TOKEN")
        .or_else(|_| env::var("GITHUB_TOKEN"))
        .expect("ACCESS_TOKEN or GITHUB_TOKEN environment variable is required");

    let username = env::var("GITHUB_ACTOR").expect("GITHUB_ACTOR environment variable is required");

    let excluded_repos: Vec<String> = env::var("EXCLUDED")
        .ok()
        .map(|s| s.split(',').map(|r| r.trim().to_string()).collect())
        .unwrap_or_default();

    let excluded_langs: Vec<String> = env::var("EXCLUDED_LANGS")
        .ok()
        .map(|s| s.split(',').map(|l| l.trim().to_string()).collect())
        .unwrap_or_default();

    let exclude_forked = env::var("EXCLUDE_FORKED_REPOS")
        .ok()
        .is_some_and(|s| s.trim().to_lowercase() != "false");

    if !excluded_repos.is_empty() {
        println!("Excluding repos: {excluded_repos:?}");
    }
    if excluded_langs.is_empty() {
        println!("Excluding languages: HTML (default)");
    } else {
        println!("Excluding languages: {excluded_langs:?} (plus HTML by default)");
    }
    if exclude_forked {
        println!("Excluding forked repositories");
    }

    // Collect statistics
    println!("Collecting GitHub statistics for {username}...");
    let stats_collector = StatsCollector::new(
        username,
        access_token,
        excluded_repos,
        excluded_langs,
        exclude_forked,
    );

    let stats = stats_collector.collect_all_stats().await?;

    // Generate SVGs
    println!("Generating SVG files...");
    let generator = SvgGenerator::new();

    generator.generate_overview(&stats).await?;
    generator.generate_languages(&stats).await?;

    println!("Successfully generated statistics!");
    Ok(())
}

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Write;

use crate::github_client::GitHubClient;

#[derive(Debug, Serialize, Deserialize)]
pub struct Stats {
    pub name: String,
    pub username: String,
    pub total_stars: u64,
    pub total_forks: u64,
    pub total_contributions: u64,
    pub total_repos: usize,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub total_views: u64,
    pub languages: HashMap<String, LanguageInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LanguageInfo {
    pub size: u64,
    pub occurrences: u32,
    pub color: Option<String>,
    pub percentage: f64,
}

#[derive(Debug, Clone)]
struct RepoLanguageEntry {
    name: String,
    size: u64,
    color: Option<String>,
}

#[derive(Debug)]
struct RepoData {
    languages: Vec<RepoLanguageEntry>,
}

#[derive(Debug)]
enum RatioResult {
    Calculated(f64),
    FallbackNoStats,
    FallbackEmptyStats,
    FallbackNoLines,
    FallbackUserNotFound,
}

pub struct StatsCollector {
    username: String,
    client: GitHubClient,
    excluded_repos: Vec<String>,
    excluded_langs: Vec<String>,
    exclude_forked: bool,
}

impl StatsCollector {
    pub fn new(
        username: &str,
        access_token: String,
        excluded_repos: Vec<String>,
        excluded_langs: &[String],
        exclude_forked: bool,
    ) -> Self {
        Self {
            username: username.to_string(),
            client: GitHubClient::new(access_token, 10),
            excluded_repos,
            excluded_langs: excluded_langs.iter().map(|s| s.to_lowercase()).collect(),
            exclude_forked,
        }
    }

    pub async fn collect_all_stats(&self) -> Result<Stats> {
        let mut stats = Stats {
            name: String::new(),
            username: self.username.clone(),
            total_stars: 0,
            total_forks: 0,
            total_contributions: 0,
            total_repos: 0,
            lines_added: 0,
            lines_deleted: 0,
            total_views: 0,
            languages: HashMap::new(),
        };

        // Phase 1: Collect repository information and raw language data
        let (repos, repo_languages) = self.collect_repos(&mut stats).await?;
        stats.total_repos = repos.len();

        // Phase 2: Fetch contributor stats for all repos (used for both
        // contribution ratios and lines changed)
        let contributor_stats = self.fetch_contributor_stats(&repos).await;

        // Phase 3: Calculate contribution ratios and apply weighted language stats
        let ratios = self.calculate_contribution_ratios(&contributor_stats, &repos);
        Self::apply_weighted_languages(&repo_languages, &ratios, &mut stats);

        // Extract lines added/deleted from contributor stats
        let (added, deleted) = self.extract_lines_changed(&contributor_stats);
        stats.lines_added = added;
        stats.lines_deleted = deleted;

        // Collect contributions and views in parallel
        let (contributions, views) = tokio::join!(
            self.collect_contributions(),
            self.collect_views(&repos)
        );

        stats.total_contributions = contributions?;

        if let Ok(total_views) = views {
            stats.total_views = total_views;
        }

        // Calculate language percentages
        let total_size: u64 = stats.languages.values().map(|l| l.size).sum();
        for lang in stats.languages.values_mut() {
            #[allow(clippy::cast_precision_loss)]
            let percentage = if total_size > 0 {
                (lang.size as f64 / total_size as f64) * 100.0
            } else {
                0.0
            };
            lang.percentage = percentage;
        }

        Ok(stats)
    }

    async fn collect_repos(
        &self,
        stats: &mut Stats,
    ) -> Result<(Vec<String>, HashMap<String, RepoData>)> {
        let mut repos = Vec::new();
        let mut repo_languages: HashMap<String, RepoData> = HashMap::new();
        let mut owned_cursor: Option<String> = None;
        let mut contrib_cursor: Option<String> = None;

        loop {
            let query = Self::build_repos_query(owned_cursor.as_deref(), contrib_cursor.as_deref());
            let response = self.client.graphql_query(&query).await?;

            let data = &response["data"]["viewer"];

            // Get name
            if stats.name.is_empty() {
                stats.name = data["name"]
                    .as_str()
                    .or_else(|| data["login"].as_str())
                    .unwrap_or("Unknown")
                    .to_string();
            }

            // Process owned repositories
            if let Some(owned) = data["repositories"].as_object() {
                if let Some(nodes) = owned["nodes"].as_array() {
                    for repo in nodes {
                        self.process_repo(repo, &mut repos, &mut repo_languages, stats);
                    }
                }

                if let Some(page_info) = owned["pageInfo"].as_object() {
                    if page_info["hasNextPage"].as_bool() == Some(true) {
                        owned_cursor = page_info["endCursor"].as_str().map(String::from);
                    } else {
                        owned_cursor = None;
                    }
                }
            }

            // Process contributed repositories (if not excluding forked)
            if !self.exclude_forked {
                if let Some(contrib) = data["repositoriesContributedTo"].as_object() {
                    if let Some(nodes) = contrib["nodes"].as_array() {
                        for repo in nodes {
                            self.process_repo(repo, &mut repos, &mut repo_languages, stats);
                        }
                    }

                    if let Some(page_info) = contrib["pageInfo"].as_object() {
                        if page_info["hasNextPage"].as_bool() == Some(true) {
                            contrib_cursor = page_info["endCursor"].as_str().map(String::from);
                        } else {
                            contrib_cursor = None;
                        }
                    }
                }
            }

            // Check if we need to continue paginating
            if owned_cursor.is_none() && contrib_cursor.is_none() {
                break;
            }
        }

        Ok((repos, repo_languages))
    }

    fn process_repo(
        &self,
        repo: &Value,
        repos: &mut Vec<String>,
        repo_languages: &mut HashMap<String, RepoData>,
        stats: &mut Stats,
    ) {
        if repo.is_null() {
            return;
        }

        let Some(name) = repo["nameWithOwner"].as_str() else {
            return;
        };

        // Skip if excluded
        if self.excluded_repos.contains(&name.to_string()) || repos.contains(&name.to_string()) {
            return;
        }

        repos.push(name.to_string());

        // Add stars and forks
        if let Some(stargazers) = repo["stargazers"]["totalCount"].as_u64() {
            stats.total_stars += stargazers;
        }
        if let Some(forks) = repo["forkCount"].as_u64() {
            stats.total_forks += forks;
        }

        // Collect raw language data (will be weighted later)
        let mut languages = Vec::new();
        if let Some(edges) = repo["languages"]["edges"].as_array() {
            for edge in edges {
                let lang_name = edge["node"]["name"].as_str().unwrap_or("Other");
                let lang_lower = lang_name.to_lowercase();

                // Always exclude HTML (often autogenerated) and any user-specified languages
                if lang_lower == "html" || self.excluded_langs.contains(&lang_lower) {
                    continue;
                }

                let size = edge["size"].as_u64().unwrap_or(0);
                let color = edge["node"]["color"].as_str().map(String::from);

                languages.push(RepoLanguageEntry {
                    name: lang_name.to_string(),
                    size,
                    color,
                });
            }
        }

        repo_languages.insert(name.to_string(), RepoData { languages });
    }

    async fn collect_contributions(&self) -> Result<u64> {
        // Get contribution years
        let years_query = r"
        query {
            viewer {
                contributionsCollection {
                    contributionYears
                }
            }
        }";

        let response = self.client.graphql_query(years_query).await?;
        let years = response["data"]["viewer"]["contributionsCollection"]["contributionYears"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Failed to get contribution years"))?;

        if years.is_empty() {
            return Ok(0);
        }

        // Build query for all years
        let mut year_queries = String::new();
        for year in years {
            if let Some(year_val) = year.as_i64() {
                write!(
                    year_queries,
                    r#"
                    year{}: contributionsCollection(
                        from: "{}-01-01T00:00:00Z",
                        to: "{}-01-01T00:00:00Z"
                    ) {{
                        contributionCalendar {{
                            totalContributions
                        }}
                    }}"#,
                    year_val,
                    year_val,
                    year_val + 1
                )?;
            }
        }

        let query = format!(
            r"
            query {{
                viewer {{
                    {year_queries}
                }}
            }}"
        );

        let response = self.client.graphql_query(&query).await?;
        let viewer = &response["data"]["viewer"];

        let mut total = 0u64;
        if let Some(obj) = viewer.as_object() {
            for (_key, value) in obj {
                if let Some(contribs) = value["contributionCalendar"]["totalContributions"].as_u64()
                {
                    total += contribs;
                }
            }
        }

        Ok(total)
    }

    async fn collect_views(&self, repos: &[String]) -> Result<u64> {
        let paths: Vec<String> = repos
            .iter()
            .map(|repo| format!("/repos/{repo}/traffic/views"))
            .collect();

        let results = self.client.rest_get_batch(paths).await;

        let mut total_views = 0u64;
        for (_path, result) in results {
            if let Ok(traffic) = result {
                if let Some(views) = traffic["views"].as_array() {
                    for view in views {
                        total_views += view["count"].as_u64().unwrap_or(0);
                    }
                }
            }
        }

        Ok(total_views)
    }

    async fn fetch_contributor_stats(&self, repos: &[String]) -> HashMap<String, Value> {
        let paths: Vec<String> = repos
            .iter()
            .map(|repo| format!("/repos/{repo}/stats/contributors"))
            .collect();

        let results = self.client.rest_get_batch(paths).await;

        let mut stats_map = HashMap::new();
        for (path, result) in results {
            if let Ok(data) = result {
                // Extract repo name from path: /repos/{owner}/{repo}/stats/contributors
                let parts: Vec<&str> = path.split('/').collect();
                if parts.len() >= 4 {
                    let repo_name = format!("{}/{}", parts[2], parts[3]);
                    stats_map.insert(repo_name, data);
                }
            }
        }

        stats_map
    }

    fn calculate_contribution_ratios(
        &self,
        contributor_stats: &HashMap<String, Value>,
        all_repos: &[String],
    ) -> HashMap<String, f64> {
        let mut ratios = HashMap::new();
        let mut weighted_count = 0u32;
        let mut fallback_count = 0u32;

        for repo_name in all_repos {
            let result = if let Some(contributors) = contributor_stats.get(repo_name) {
                self.calculate_single_ratio(repo_name, contributors)
            } else {
                println!("  [fallback] {repo_name}: no contributor stats available");
                RatioResult::FallbackNoStats
            };

            let ratio = if let RatioResult::Calculated(r) = &result {
                weighted_count += 1;
                *r
            } else {
                fallback_count += 1;
                1.0
            };

            ratios.insert(repo_name.clone(), ratio);
        }

        let total = weighted_count + fallback_count;
        println!(
            "Weighted {weighted_count}/{total} repos by contribution ratio ({fallback_count} fell back to 100%)"
        );

        ratios
    }

    fn calculate_single_ratio(&self, repo_name: &str, contributors: &Value) -> RatioResult {
        let Some(contrib_array) = contributors.as_array() else {
            println!("  [fallback] {repo_name}: contributor stats not an array");
            return RatioResult::FallbackNoStats;
        };

        if contrib_array.is_empty() {
            println!("  [fallback] {repo_name}: empty contributor stats");
            return RatioResult::FallbackEmptyStats;
        }

        let mut my_added: u64 = 0;
        let mut total_added: u64 = 0;
        let mut found_user = false;

        for contributor in contrib_array {
            let added: u64 = contributor["weeks"]
                .as_array()
                .map_or(0, |weeks| weeks.iter().map(|w| w["a"].as_u64().unwrap_or(0)).sum());

            total_added += added;

            if let Some(author) = contributor["author"]["login"].as_str() {
                if author.eq_ignore_ascii_case(&self.username) {
                    my_added = added;
                    found_user = true;
                }
            }
        }

        if total_added == 0 {
            println!("  [fallback] {repo_name}: no lines added by any contributor");
            return RatioResult::FallbackNoLines;
        }

        if !found_user {
            println!(
                "  [fallback] {}: user '{}' not found in contributors",
                repo_name, self.username
            );
            return RatioResult::FallbackUserNotFound;
        }

        #[allow(clippy::cast_precision_loss)]
        let ratio = (my_added as f64 / total_added as f64).min(1.0);
        RatioResult::Calculated(ratio)
    }

    fn apply_weighted_languages(
        repo_languages: &HashMap<String, RepoData>,
        ratios: &HashMap<String, f64>,
        stats: &mut Stats,
    ) {
        for (repo_name, repo_data) in repo_languages {
            let ratio = ratios.get(repo_name).copied().unwrap_or(1.0);

            for lang_entry in &repo_data.languages {
                // Precision loss is acceptable for byte counts; truncation is intentional
                // (rounding a positive value), and sign loss cannot occur (ratio >= 0)
                #[allow(
                    clippy::cast_precision_loss,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss
                )]
                let weighted_size = (lang_entry.size as f64 * ratio).round() as u64;

                let entry = stats
                    .languages
                    .entry(lang_entry.name.clone())
                    .or_insert(LanguageInfo {
                        size: 0,
                        occurrences: 0,
                        color: lang_entry.color.clone(),
                        percentage: 0.0,
                    });

                entry.size += weighted_size;
                entry.occurrences += 1;
            }
        }
    }

    fn extract_lines_changed(&self, contributor_stats: &HashMap<String, Value>) -> (u64, u64) {
        let mut total_added = 0u64;
        let mut total_deleted = 0u64;

        for contributors in contributor_stats.values() {
            if let Some(contrib_array) = contributors.as_array() {
                for contributor in contrib_array {
                    if let Some(author) = contributor["author"]["login"].as_str() {
                        if author.eq_ignore_ascii_case(&self.username) {
                            if let Some(weeks) = contributor["weeks"].as_array() {
                                for week in weeks {
                                    total_added += week["a"].as_u64().unwrap_or(0);
                                    total_deleted += week["d"].as_u64().unwrap_or(0);
                                }
                            }
                        }
                    }
                }
            }
        }

        (total_added, total_deleted)
    }

    fn build_repos_query(owned_cursor: Option<&str>, contrib_cursor: Option<&str>) -> String {
        format!(
            r"{{
                viewer {{
                    login,
                    name,
                    repositories(
                        first: 100,
                        orderBy: {{field: UPDATED_AT, direction: DESC}},
                        isFork: false,
                        after: {}
                    ) {{
                        pageInfo {{
                            hasNextPage
                            endCursor
                        }}
                        nodes {{
                            nameWithOwner
                            stargazers {{
                                totalCount
                            }}
                            forkCount
                            languages(first: 10, orderBy: {{field: SIZE, direction: DESC}}) {{
                                edges {{
                                    size
                                    node {{
                                        name
                                        color
                                    }}
                                }}
                            }}
                        }}
                    }}
                    repositoriesContributedTo(
                        first: 100,
                        includeUserRepositories: false,
                        orderBy: {{field: UPDATED_AT, direction: DESC}},
                        contributionTypes: [COMMIT, PULL_REQUEST, REPOSITORY, PULL_REQUEST_REVIEW]
                        after: {}
                    ) {{
                        pageInfo {{
                            hasNextPage
                            endCursor
                        }}
                        nodes {{
                            nameWithOwner
                            stargazers {{
                                totalCount
                            }}
                            forkCount
                            languages(first: 10, orderBy: {{field: SIZE, direction: DESC}}) {{
                                edges {{
                                    size
                                    node {{
                                        name
                                        color
                                    }}
                                }}
                            }}
                        }}
                    }}
                }}
            }}",
            owned_cursor.map_or_else(|| "null".to_string(), |c| format!(r#""{c}""#)),
            contrib_cursor.map_or_else(|| "null".to_string(), |c| format!(r#""{c}""#))
        )
    }
}

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

        // Collect repository information
        let repos = self.collect_repos(&mut stats).await?;
        stats.total_repos = repos.len();

        // Collect contributions
        stats.total_contributions = self.collect_contributions().await?;

        // Collect lines changed and views in parallel
        let (lines_changed, views) = tokio::join!(
            self.collect_lines_changed(&repos),
            self.collect_views(&repos)
        );

        if let Ok((added, deleted)) = lines_changed {
            stats.lines_added = added;
            stats.lines_deleted = deleted;
        }

        if let Ok(total_views) = views {
            stats.total_views = total_views;
        }

        // Calculate language percentages
        let total_size: u64 = stats.languages.values().map(|l| l.size).sum();
        for lang in stats.languages.values_mut() {
            lang.percentage = if total_size > 0 {
                (lang.size as f64 / total_size as f64) * 100.0
            } else {
                0.0
            };
        }

        Ok(stats)
    }

    async fn collect_repos(&self, stats: &mut Stats) -> Result<Vec<String>> {
        let mut repos = Vec::new();
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
                        self.process_repo(repo, &mut repos, stats);
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
                            self.process_repo(repo, &mut repos, stats);
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

        Ok(repos)
    }

    fn process_repo(&self, repo: &Value, repos: &mut Vec<String>, stats: &mut Stats) {
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

        // Process languages
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

                let entry = stats
                    .languages
                    .entry(lang_name.to_string())
                    .or_insert(LanguageInfo {
                        size: 0,
                        occurrences: 0,
                        color,
                        percentage: 0.0,
                    });

                entry.size += size;
                entry.occurrences += 1;
            }
        }
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

    async fn collect_lines_changed(&self, repos: &[String]) -> Result<(u64, u64)> {
        let paths: Vec<String> = repos
            .iter()
            .map(|repo| format!("/repos/{repo}/stats/contributors"))
            .collect();

        let results = self.client.rest_get_batch(paths).await;

        let mut total_added = 0u64;
        let mut total_deleted = 0u64;

        for (_path, result) in results {
            if let Ok(contributors) = result {
                if let Some(contrib_array) = contributors.as_array() {
                    for contributor in contrib_array {
                        if let Some(author) = contributor["author"]["login"].as_str() {
                            if author == self.username {
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
        }

        Ok((total_added, total_deleted))
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
                        contributionTypes: [COMMIT, PULL_REQUEST, PULL_REQUEST_REVIEW]
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

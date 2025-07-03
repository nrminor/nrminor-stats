use anyhow::{anyhow, Result};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{sleep, Duration};

use crate::cache::Cache;

pub struct GitHubClient {
    client: Client,
    access_token: String,
    semaphore: Arc<Semaphore>,
    cache: Cache,
}

impl GitHubClient {
    pub fn new(access_token: String, max_concurrent_requests: usize) -> Self {
        let client = Client::builder()
            .user_agent("github-stats-generator")
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            access_token,
            semaphore: Arc::new(Semaphore::new(max_concurrent_requests)),
            cache: Cache::new(".github_stats_cache", 6),
        }
    }

    pub async fn graphql_query(&self, query: &str) -> Result<Value> {
        let _permit = self.semaphore.acquire().await?;

        let response = self
            .client
            .post("https://api.github.com/graphql")
            .header("Authorization", format!("Bearer {}", self.access_token))
            .json(&json!({ "query": query }))
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_else(|_| "No error body".to_string());
            return Err(anyhow!(
                "GraphQL query failed with status: {}. Body: {}",
                status,
                error_body
            ));
        }

        let data: Value = response.json().await?;
        Ok(data)
    }

    pub async fn rest_get(&self, path: &str) -> Result<Value> {
        let cache_key = format!("rest:{}", path);
        
        // Check cache first
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(cached);
        }

        let url = if path.starts_with('/') {
            format!("https://api.github.com{}", path)
        } else {
            format!("https://api.github.com/{}", path)
        };

        let mut retries = 0;
        const MAX_RETRIES: u32 = 10;

        loop {
            let _permit = self.semaphore.acquire().await?;
            
            let response = self
                .client
                .get(&url)
                .header("Authorization", format!("token {}", self.access_token))
                .send()
                .await?;

            match response.status() {
                StatusCode::OK => {
                    let data: Value = response.json().await?;
                    // Cache successful response
                    self.cache.set(&cache_key, &data)?;
                    return Ok(data);
                }
                StatusCode::ACCEPTED => {
                    // 202 means data is being calculated, retry
                    retries += 1;
                    if retries >= MAX_RETRIES {
                        return Err(anyhow!("Too many retries for {}", path));
                    }
                    if retries == 5 || retries == MAX_RETRIES {
                        println!("Still waiting for {} statistics (attempt {}/{})", 
                            path.split('/').nth(2).unwrap_or("repo"), 
                            retries, 
                            MAX_RETRIES
                        );
                    }
                    drop(_permit); // Release semaphore before sleeping
                    sleep(Duration::from_secs(1)).await;
                }
                _ => {
                    return Err(anyhow!(
                        "REST API request failed with status: {}",
                        response.status()
                    ));
                }
            }
        }
    }

    pub async fn rest_get_batch(&self, paths: Vec<String>) -> Vec<(String, Result<Value>)> {
        let mut handles = vec![];

        for path in paths {
            let client = self.clone();
            let handle = tokio::spawn(async move {
                let result = client.rest_get(&path).await;
                (path, result)
            });
            handles.push(handle);
        }

        let mut results = vec![];
        for handle in handles {
            if let Ok(result) = handle.await {
                results.push(result);
            }
        }

        results
    }
}

impl Clone for GitHubClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            access_token: self.access_token.clone(),
            semaphore: Arc::clone(&self.semaphore),
            cache: self.cache.clone(),
        }
    }
}
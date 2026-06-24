use crate::log_parser::ParsedEvent;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

const SYNC_INTERVAL_SECS: u64 = 10;
const MAX_BATCH_SIZE: usize = 200;
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct SyncEngine {
    queue: Arc<Mutex<Vec<ParsedEvent>>>,
    api_key: Arc<Mutex<String>>,
    api_base_url: Arc<Mutex<String>>,
    pub last_error: Arc<Mutex<Option<String>>>,
    pub last_sync: Arc<Mutex<Option<String>>>,
    pub syncing: Arc<Mutex<bool>>,
}

impl SyncEngine {
    pub fn new(api_key: String, api_base_url: String) -> Self {
        Self {
            queue: Arc::new(Mutex::new(Vec::new())),
            api_key: Arc::new(Mutex::new(api_key)),
            api_base_url: Arc::new(Mutex::new(api_base_url)),
            last_error: Arc::new(Mutex::new(None)),
            last_sync: Arc::new(Mutex::new(None)),
            syncing: Arc::new(Mutex::new(false)),
        }
    }

    pub async fn enqueue(&self, event: ParsedEvent) {
        let mut q = self.queue.lock().await;
        q.push(event);
    }

    pub async fn pending_count(&self) -> usize {
        self.queue.lock().await.len()
    }

    pub async fn update_credentials(&self, api_key: String, api_base_url: String) {
        *self.api_key.lock().await = api_key;
        *self.api_base_url.lock().await = api_base_url;
    }

    pub fn start_background_sync(self) {
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(SYNC_INTERVAL_SECS)).await;
                self.flush_once().await;
            }
        });
    }

    async fn flush_once(&self) {
        let batch: Vec<ParsedEvent> = {
            let mut q = self.queue.lock().await;
            if q.is_empty() {
                return;
            }
            let n = q.len().min(MAX_BATCH_SIZE);
            q.drain(..n).collect()
        };

        let api_key = self.api_key.lock().await.clone();
        let api_base_url = self.api_base_url.lock().await.clone();

        if api_key.is_empty() || api_base_url.is_empty() {
            let mut q = self.queue.lock().await;
            for ev in batch.into_iter().rev() { q.insert(0, ev); }
            return;
        }

        *self.syncing.lock().await = true;

        let result = post_batch(&api_key, &api_base_url, &batch).await;

        *self.syncing.lock().await = false;

        match result {
            Ok(_) => {
                *self.last_error.lock().await = None;
                let now = chrono::Utc::now().format("%H:%M:%S").to_string();
                *self.last_sync.lock().await = Some(now);
            }
            Err(e) => {
                // Retry once
                let retry = post_batch(&api_key, &api_base_url, &batch).await;
                match retry {
                    Ok(_) => {
                        *self.last_error.lock().await = None;
                        let now = chrono::Utc::now().format("%H:%M:%S").to_string();
                        *self.last_sync.lock().await = Some(now);
                    }
                    Err(_) => {
                        *self.last_error.lock().await = Some(format!("Sync failed: {}", e));
                        // Re-queue events on failure
                        let mut q = self.queue.lock().await;
                        for ev in batch.into_iter().rev() { q.insert(0, ev); }
                    }
                }
            }
        }
    }
}

async fn post_batch(api_key: &str, api_base_url: &str, batch: &[ParsedEvent]) -> Result<(), String> {
    let url = format!("{}/partner/sync", api_base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let body = json!({
        "events": batch,
        "clientVersion": CLIENT_VERSION,
    });

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        Err(format!("HTTP {}: {}", status, text))
    }
}

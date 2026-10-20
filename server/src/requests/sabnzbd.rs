//! SABnzbd queue. `addurl` is the only way a request becomes a job.

use crate::requests::http::{service_text, Upstream, UpstreamError};
use reqwest::Url;
use serde_json::Value;
use std::path::Path;

pub(crate) struct Sabnzbd<'a> {
    pub up: &'a Upstream,
    pub base: Url,
    pub key: String,
    pub category: String,
}

pub(crate) enum Job {
    /// Still in the queue. `progress` is 0..100.
    Queued {
        progress: u8,
    },
    /// History: Verifying, Repairing, Extracting, Moving, Running, or Queued.
    Processing,
    /// `folder` is `file_name(history.storage)`. The worker checks it.
    Completed {
        folder: String,
    },
    Failed {
        message: String,
    },
    Missing,
}

impl Sabnzbd<'_> {
    /// `mode=addurl&name=<downloadUrl>&nzbname=mewo-<request id>&cat=<category>`.
    pub async fn add_url(&self, url: &str, job_name: &str) -> Result<String, UpstreamError> {
        let value = self
            .get(&[
                ("mode", "addurl"),
                ("name", url),
                ("nzbname", job_name),
                ("cat", self.category.as_str()),
                ("output", "json"),
                ("apikey", self.key.as_str()),
            ])
            .await?;
        sab_error(&value)?;
        let ids = value
            .get("nzo_ids")
            .and_then(Value::as_array)
            .ok_or(UpstreamError::Shape)?;
        let id = ids
            .first()
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or(UpstreamError::Shape)?;
        Ok(id.to_string())
    }

    /// Queue + history `search=<job_name>`. Used before `add_url` so a crash after
    /// add but before our UPDATE doesn't grab twice.
    pub async fn find_by_name(&self, job_name: &str) -> Result<Option<String>, UpstreamError> {
        if let Some(id) = self
            .slot_id("queue", &[("search", job_name)], job_name)
            .await?
        {
            return Ok(Some(id));
        }
        self.slot_id("history", &[("search", job_name)], job_name)
            .await
    }

    pub async fn job(&self, nzo_id: &str) -> Result<Job, UpstreamError> {
        let queued = self.slots("queue", &[("nzo_ids", nzo_id)]).await?;
        if let Some(slot) = queued.into_iter().find(|slot| slot_id_is(slot, nzo_id)) {
            return Ok(Job::Queued {
                progress: percent(slot.get("percentage")),
            });
        }
        let history = self.slots("history", &[("nzo_ids", nzo_id)]).await?;
        let Some(slot) = history.into_iter().find(|slot| slot_id_is(slot, nzo_id)) else {
            return Ok(Job::Missing);
        };
        Ok(history_job(&slot))
    }

    /// Queue delete and history delete. `archive=0` deletes a history row
    /// instead of archiving it. `del_files=1` only removes an incomplete
    /// folder; the worker deletes a completed one. A missing job is `Ok`.
    pub async fn delete(&self, nzo_id: &str) -> Result<(), UpstreamError> {
        self.delete_mode("queue", nzo_id).await?;
        self.delete_mode("history", nzo_id).await?;
        Ok(())
    }

    /// `mode=version`, then `get_cats` must contain `category`.
    pub async fn status(&self) -> Result<String, UpstreamError> {
        let version = self
            .get(&[
                ("mode", "version"),
                ("output", "json"),
                ("apikey", self.key.as_str()),
            ])
            .await?;
        sab_error(&version)?;
        let version = version
            .get("version")
            .and_then(Value::as_str)
            .filter(|version| !version.is_empty())
            .ok_or(UpstreamError::Shape)?;
        let cats = self
            .get(&[
                ("mode", "get_cats"),
                ("output", "json"),
                ("apikey", self.key.as_str()),
            ])
            .await?;
        sab_error(&cats)?;
        if !category_names(&cats)
            .iter()
            .any(|name| name == &self.category)
        {
            return Err(UpstreamError::Service(format!(
                "No category \"{}\"",
                self.category
            )));
        }
        Ok(format!("{version} · category {}", self.category))
    }

    async fn delete_mode(&self, mode: &'static str, nzo_id: &str) -> Result<(), UpstreamError> {
        let value = self
            .get(&[
                ("mode", mode),
                ("name", "delete"),
                ("value", nzo_id),
                ("del_files", "1"),
                ("archive", "0"),
                ("output", "json"),
                ("apikey", self.key.as_str()),
            ])
            .await?;
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            let text = error.to_ascii_lowercase();
            if text.contains("missing") || text.contains("not found") || text.contains("no such") {
                return Ok(());
            }
            return Err(UpstreamError::Service(service_text(error)));
        }
        Ok(())
    }

    async fn slot_id(
        &self,
        mode: &'static str,
        query: &[(&str, &str)],
        job_name: &str,
    ) -> Result<Option<String>, UpstreamError> {
        let slots = self.slots(mode, query).await?;
        Ok(slots.into_iter().find_map(|slot| {
            let name = slot
                .get("filename")
                .or_else(|| slot.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if name == job_name {
                slot.get("nzo_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            } else {
                None
            }
        }))
    }

    async fn slots(
        &self,
        mode: &'static str,
        extra: &[(&str, &str)],
    ) -> Result<Vec<Value>, UpstreamError> {
        let mut query = vec![
            ("mode", mode),
            ("output", "json"),
            ("apikey", self.key.as_str()),
        ];
        query.extend_from_slice(extra);
        let value = self.get(&query).await?;
        sab_error(&value)?;
        let key = if mode == "history" {
            "history"
        } else {
            "queue"
        };
        let slots = value
            .get(key)
            .and_then(|section| section.get("slots"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(slots)
    }

    async fn get(&self, query: &[(&str, &str)]) -> Result<Value, UpstreamError> {
        self.up
            .get_json(&self.base, "/api", query, &[], 1024 * 1024)
            .await
    }
}

fn history_job(slot: &Value) -> Job {
    let status = slot.get("status").and_then(Value::as_str).unwrap_or("");
    match status.to_ascii_lowercase().as_str() {
        "completed" => Job::Completed {
            folder: folder_name(slot.get("storage").and_then(Value::as_str).unwrap_or("")),
        },
        "failed" => Job::Failed {
            message: slot
                .get("fail_message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        },
        "verifying" | "repairing" | "extracting" | "moving" | "running" | "queued" => {
            Job::Processing
        }
        _ => Job::Processing,
    }
}

pub(crate) fn folder_name(storage: &str) -> String {
    Path::new(storage)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string()
}

fn slot_id_is(slot: &Value, nzo_id: &str) -> bool {
    slot.get("nzo_id").and_then(Value::as_str) == Some(nzo_id)
}

fn percent(value: Option<&Value>) -> u8 {
    let number = match value {
        Some(Value::String(text)) => text.parse::<f64>().unwrap_or(0.0),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(0.0),
        _ => 0.0,
    };
    number.clamp(0.0, 100.0) as u8
}

fn sab_error(value: &Value) -> Result<(), UpstreamError> {
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        if !error.is_empty() {
            return Err(UpstreamError::Service(service_text(error)));
        }
    }
    if value.get("status").and_then(Value::as_bool) == Some(false) {
        return Err(UpstreamError::Service("error".into()));
    }
    Ok(())
}

fn category_names(value: &Value) -> Vec<String> {
    let Some(categories) = value.get("categories").and_then(Value::as_array) else {
        return Vec::new();
    };
    categories
        .iter()
        .filter_map(|item| match item {
            Value::String(name) => Some(name.clone()),
            Value::Object(obj) => obj.get("name").and_then(Value::as_str).map(str::to_string),
            _ => None,
        })
        .collect()
}

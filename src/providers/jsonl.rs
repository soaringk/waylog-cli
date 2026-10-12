use crate::error::Result;
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Append-only logs can end in a record the provider has not finished writing.
pub(super) struct JsonlReader {
    reader: BufReader<File>,
    source: PathBuf,
}

impl JsonlReader {
    pub(super) async fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            reader: BufReader::new(File::open(path).await?),
            source: path.to_path_buf(),
        })
    }

    pub(super) async fn next<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        let mut line = String::new();
        loop {
            line.clear();
            if self.reader.read_line(&mut line).await? == 0 {
                return Ok(None);
            }
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str(&line) {
                Ok(record) => return Ok(Some(record)),
                Err(error) if error.is_eof() && !line.ends_with('\n') => {
                    tracing::warn!(path = %self.source.display(), "Ignoring unfinished final JSONL record");
                    return Ok(None);
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::TempDir;

    #[tokio::test]
    async fn accepts_complete_unterminated_record_and_retries_a_growing_tail() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("live.jsonl");
        tokio::fs::write(&file, "{\"id\":1}\n{\"id\":")
            .await
            .unwrap();
        let mut reader = JsonlReader::open(&file).await.unwrap();
        assert_eq!(reader.next::<Value>().await.unwrap().unwrap()["id"], 1);
        assert!(reader.next::<Value>().await.unwrap().is_none());
        tokio::fs::write(&file, "{\"id\":1}\n{\"id\":2}")
            .await
            .unwrap();
        let mut reader = JsonlReader::open(&file).await.unwrap();
        reader.next::<Value>().await.unwrap();
        assert_eq!(reader.next::<Value>().await.unwrap().unwrap()["id"], 2);
    }

    #[tokio::test]
    async fn rejects_corrupt_or_committed_incomplete_records() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("bad.jsonl");
        for content in [
            "{\"id\":\n{\"id\":2}\n",
            "{\"id\":\n",
            "not json",
            "{\"id\":false,}",
        ] {
            tokio::fs::write(&file, content).await.unwrap();
            assert!(JsonlReader::open(&file)
                .await
                .unwrap()
                .next::<Value>()
                .await
                .is_err());
        }
    }
}

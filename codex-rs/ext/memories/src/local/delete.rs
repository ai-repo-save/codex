use chrono::Utc;
use sha2::Digest;
use sha2::Sha256;

use crate::backend::DeleteMemoryRequest;
use crate::backend::DeleteMemoryResponse;
use crate::backend::MemoriesBackendError;

use super::LocalMemoriesBackend;
use super::path::reject_symlink;

const DELETED_DIR: &str = ".deleted";

pub(super) async fn delete(
    backend: &LocalMemoriesBackend,
    request: DeleteMemoryRequest,
) -> Result<DeleteMemoryResponse, MemoriesBackendError> {
    let path = backend
        .resolve_scoped_path(Some(request.path.as_str()))
        .await?;
    let Some(metadata) = LocalMemoriesBackend::metadata_or_none(&path).await? else {
        return Err(MemoriesBackendError::NotFound { path: request.path });
    };
    reject_symlink(&request.path, &metadata)?;
    if !metadata.is_file() {
        return Err(MemoriesBackendError::NotFile { path: request.path });
    }

    let deleted_dir = ensure_deleted_dir(backend).await?;
    move_to_deleted(&path, &deleted_dir, &request.path).await?;

    Ok(DeleteMemoryResponse {
        path: request.path,
        deleted: true,
    })
}

fn deleted_filename(path: &str) -> String {
    let timestamp = Utc::now().format("%Y-%m-%dT%H-%M-%S%.fZ");
    let digest = Sha256::digest(path.as_bytes());
    let hash = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let original_name = path.rsplit('/').next().unwrap_or("memory");
    format!("{timestamp}-{hash}-{original_name}")
}

async fn ensure_deleted_dir(
    backend: &LocalMemoriesBackend,
) -> Result<std::path::PathBuf, MemoriesBackendError> {
    match LocalMemoriesBackend::metadata_or_none(&backend.root).await? {
        Some(metadata) => {
            reject_symlink(&backend.root.display().to_string(), &metadata)?;
            if !metadata.is_dir() {
                return Err(MemoriesBackendError::invalid_path(
                    backend.root.display().to_string(),
                    "must be a directory",
                ));
            }
        }
        None => tokio::fs::create_dir(&backend.root).await?,
    }

    let deleted_dir = backend.root.join(DELETED_DIR);
    match LocalMemoriesBackend::metadata_or_none(&deleted_dir).await? {
        Some(metadata) => {
            reject_symlink(&deleted_dir.display().to_string(), &metadata)?;
            if !metadata.is_dir() {
                return Err(MemoriesBackendError::invalid_path(
                    deleted_dir.display().to_string(),
                    "must be a directory",
                ));
            }
        }
        None => tokio::fs::create_dir(&deleted_dir).await?,
    }
    Ok(deleted_dir)
}

async fn move_to_deleted(
    path: &std::path::Path,
    deleted_dir: &std::path::Path,
    requested_path: &str,
) -> Result<(), MemoriesBackendError> {
    for attempt in 0..100 {
        let filename = if attempt == 0 {
            deleted_filename(requested_path)
        } else {
            format!("{}-{attempt}", deleted_filename(requested_path))
        };
        let deleted_path = deleted_dir.join(filename);
        let result = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&deleted_path)
            .await;
        let file = match result {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        };
        drop(file);
        if let Err(err) = tokio::fs::copy(path, &deleted_path).await {
            let _ = tokio::fs::remove_file(&deleted_path).await;
            return Err(err.into());
        }
        tokio::fs::remove_file(path).await?;
        return Ok(());
    }

    Err(MemoriesBackendError::AdHocNoteAlreadyExists {
        filename: deleted_filename(requested_path),
    })
}

use std::path::PathBuf;

use sqlx::PgPool;
use uuid::Uuid;

use crate::features::media::service::{
    DeleteMediaObjectInput, MediaObjectStore, PutMediaObjectInput,
};

pub const PAPER_PDF_FILE_SIZE_LIMIT_BYTES: u64 = 100 * 1024 * 1024;

use super::{
    grobid::{GrobidClient, GrobidError, GrobidPaperMetadata, PaperMetadataExtractor},
    repository::{InsertPaperMetadata, PaperMetadata, PaperRepository},
};

#[derive(Debug)]
pub enum PaperImportServiceError {
    InvalidInput,
    ObjectStoreFailed,
    Database(sqlx::Error),
    Grobid(GrobidError),
    ConflictResolutionFailed,
    NotFound,
}

impl From<sqlx::Error> for PaperImportServiceError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Debug, Clone)]
pub struct ImportPaperPdfInput {
    pub bucket: String,
    pub uploaded_by: Uuid,
    pub original_filename: Option<String>,
    pub content_type: String,
    pub file_path: PathBuf,
    pub size_bytes: u64,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportPaperPdfStatus {
    Imported,
    AlreadyImported,
    MetadataRequired,
}

#[derive(Debug, Clone)]
pub struct ImportPaperPdfOutput {
    pub status: ImportPaperPdfStatus,
    pub paper_id: Uuid,
    pub bucket: String,
    pub object_key: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub original_filename: Option<String>,
    pub sha256: String,
    pub doi: Option<String>,
    pub title: Option<String>,
    pub authors: Option<String>,
    pub publication_year: Option<i32>,
    pub journal: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub article_number: Option<String>,
    pub requires_bibliographic_input: bool,
    pub already_imported: bool,
}

#[derive(Debug, Clone)]
pub struct CompleteBibliographicMetadataInput {
    pub paper_id: Uuid,
    pub doi: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CompleteBibliographicMetadataOutput {
    pub paper_id: Uuid,
    pub doi: Option<String>,
    pub title: Option<String>,
    pub requires_bibliographic_input: bool,
}

#[async_trait::async_trait]
trait PaperImportRepository: Send + Sync {
    async fn find_by_sha256(&self, sha256: &str) -> Result<Option<PaperMetadata>, sqlx::Error>;
    async fn insert_if_sha256_absent(
        &self,
        metadata: InsertPaperMetadata<'_>,
    ) -> Result<bool, sqlx::Error>;
}

struct PgPaperImportRepository<'a> {
    db: &'a PgPool,
}

#[async_trait::async_trait]
impl PaperImportRepository for PgPaperImportRepository<'_> {
    async fn find_by_sha256(&self, sha256: &str) -> Result<Option<PaperMetadata>, sqlx::Error> {
        PaperRepository::find_by_sha256(self.db, sha256).await
    }

    async fn insert_if_sha256_absent(
        &self,
        metadata: InsertPaperMetadata<'_>,
    ) -> Result<bool, sqlx::Error> {
        PaperRepository::insert_if_sha256_absent(self.db, metadata).await
    }
}

pub struct PaperImportService;

impl PaperImportService {
    pub async fn import_pdf<S>(
        input: ImportPaperPdfInput,
        store: &S,
        db: &PgPool,
    ) -> Result<ImportPaperPdfOutput, PaperImportServiceError>
    where
        S: MediaObjectStore + ?Sized,
    {
        let input = normalize_import_input(input)?;
        let repository = PgPaperImportRepository { db };

        // PostgreSQL alone can answer duplicate imports. Delay GROBID client
        // construction so a duplicate does not depend on external configuration.
        if let Some(existing) = repository.find_by_sha256(&input.payload_sha256).await? {
            return Ok(output_from_metadata(
                ImportPaperPdfStatus::AlreadyImported,
                existing,
            ));
        }

        let grobid = GrobidClient::from_env().map_err(PaperImportServiceError::Grobid)?;
        Self::import_new_pdf_with_dependencies(input, store, &repository, &grobid).await
    }

    #[cfg(test)]
    async fn import_pdf_with_dependencies<S, R, E>(
        input: ImportPaperPdfInput,
        store: &S,
        repository: &R,
        extractor: &E,
    ) -> Result<ImportPaperPdfOutput, PaperImportServiceError>
    where
        S: MediaObjectStore + ?Sized,
        R: PaperImportRepository + ?Sized,
        E: PaperMetadataExtractor + ?Sized,
    {
        let input = normalize_import_input(input)?;

        if let Some(existing) = repository.find_by_sha256(&input.payload_sha256).await? {
            return Ok(output_from_metadata(
                ImportPaperPdfStatus::AlreadyImported,
                existing,
            ));
        }

        Self::import_new_pdf_with_dependencies(input, store, repository, extractor).await
    }

    async fn import_new_pdf_with_dependencies<S, R, E>(
        input: ImportPaperPdfInput,
        store: &S,
        repository: &R,
        extractor: &E,
    ) -> Result<ImportPaperPdfOutput, PaperImportServiceError>
    where
        S: MediaObjectStore + ?Sized,
        R: PaperImportRepository + ?Sized,
        E: PaperMetadataExtractor + ?Sized,
    {
        let ImportPaperPdfInput {
            bucket,
            uploaded_by,
            original_filename,
            content_type,
            file_path,
            size_bytes: file_size_bytes,
            payload_sha256: sha256,
        } = input;
        let database_size_bytes =
            i64::try_from(file_size_bytes).expect("paper size was validated before side effects");
        let paper_id = Uuid::new_v4();
        let object_key = format!("papers/{paper_id}/original.pdf");

        store
            .put_object(PutMediaObjectInput {
                bucket: bucket.clone(),
                object_key: object_key.clone(),
                content_type: content_type.clone(),
                file_path: file_path.clone(),
                size_bytes: file_size_bytes,
                payload_sha256: sha256.clone(),
            })
            .await
            .map_err(|_| PaperImportServiceError::ObjectStoreFailed)?;

        let grobid_metadata = match extractor.extract_header(&file_path, file_size_bytes).await {
            Ok(metadata) => metadata,
            Err(error) => {
                rollback_object(store, &bucket, &object_key).await?;
                return Err(PaperImportServiceError::Grobid(error));
            }
        };

        let insert_result = repository
            .insert_if_sha256_absent(InsertPaperMetadata {
                id: paper_id,
                bucket: &bucket,
                object_key: &object_key,
                content_type: &content_type,
                size_bytes: database_size_bytes,
                original_filename: original_filename.as_deref(),
                sha256: &sha256,
                doi: grobid_metadata.doi.as_deref(),
                title: grobid_metadata.title.as_deref(),
                authors: grobid_metadata.authors.as_deref(),
                publication_year: grobid_metadata.publication_year,
                journal: grobid_metadata.journal.as_deref(),
                volume: grobid_metadata.volume.as_deref(),
                issue: grobid_metadata.issue.as_deref(),
                pages: grobid_metadata.pages.as_deref(),
                article_number: grobid_metadata.article_number.as_deref(),
                uploaded_by,
            })
            .await;

        let inserted = match insert_result {
            Ok(inserted) => inserted,
            Err(database_error) => {
                rollback_object(store, &bucket, &object_key).await?;
                return Err(PaperImportServiceError::Database(database_error));
            }
        };

        if inserted {
            return Ok(output_from_new_import(
                paper_id,
                &bucket,
                object_key,
                &content_type,
                database_size_bytes,
                original_filename,
                sha256,
                grobid_metadata,
            ));
        }

        // The UNIQUE constraint resolved a concurrent upload first. Remove this
        // request's object and return the row that won the database race.
        rollback_object(store, &bucket, &object_key).await?;

        let existing = repository
            .find_by_sha256(&sha256)
            .await?
            .ok_or(PaperImportServiceError::ConflictResolutionFailed)?;

        Ok(output_from_metadata(
            ImportPaperPdfStatus::AlreadyImported,
            existing,
        ))
    }

    pub async fn complete_bibliographic_metadata(
        input: CompleteBibliographicMetadataInput,
        db: &PgPool,
    ) -> Result<CompleteBibliographicMetadataOutput, PaperImportServiceError> {
        let doi = input
            .doi
            .map(super::grobid::normalize_doi)
            .filter(|value| !value.is_empty());
        let title = input
            .title
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        // At least one user-provided value is required even when the paper
        // already has metadata. This prevents an empty PATCH from looking like
        // a successful completion.
        if doi.is_none() && title.is_none() {
            return Err(PaperImportServiceError::InvalidInput);
        }

        // paperはSHA-256で全ユーザー共通のため、認証済みユーザーなら補完できる。
        // repository側の原子的なUPDATEが既存値の上書きを防止する。
        let metadata = PaperRepository::complete_missing_bibliographic_metadata(
            db,
            input.paper_id,
            doi.as_deref(),
            title.as_deref(),
        )
        .await?
        .ok_or(PaperImportServiceError::NotFound)?;

        Ok(CompleteBibliographicMetadataOutput {
            paper_id: metadata.id,
            requires_bibliographic_input: requires_bibliographic_input(
                metadata.doi.as_deref(),
                metadata.title.as_deref(),
            ),
            doi: metadata.doi,
            title: metadata.title,
        })
    }
}

fn normalize_import_input(
    mut input: ImportPaperPdfInput,
) -> Result<ImportPaperPdfInput, PaperImportServiceError> {
    input.bucket = input.bucket.trim().to_string();
    input.content_type = input.content_type.trim().to_ascii_lowercase();
    input.payload_sha256 = input.payload_sha256.trim().to_ascii_lowercase();

    // PostgreSQL stores the byte count as BIGINT. Reject values that cannot be
    // represented instead of allowing a wrapping u64-to-i64 cast.
    if input.bucket.is_empty()
        || input.content_type != "application/pdf"
        || input.size_bytes == 0
        || input.size_bytes > PAPER_PDF_FILE_SIZE_LIMIT_BYTES
        || input.size_bytes > i64::MAX as u64
        || !is_valid_sha256_hex(&input.payload_sha256)
    {
        return Err(PaperImportServiceError::InvalidInput);
    }

    Ok(input)
}

async fn rollback_object<S>(
    store: &S,
    bucket: &str,
    object_key: &str,
) -> Result<(), PaperImportServiceError>
where
    S: MediaObjectStore + ?Sized,
{
    store
        .delete_object(DeleteMediaObjectInput {
            bucket: bucket.to_string(),
            object_key: object_key.to_string(),
        })
        .await
        .map_err(|_| PaperImportServiceError::ObjectStoreFailed)
}

fn output_from_new_import(
    paper_id: Uuid,
    bucket: &str,
    object_key: String,
    content_type: &str,
    size_bytes: i64,
    original_filename: Option<String>,
    sha256: String,
    metadata: GrobidPaperMetadata,
) -> ImportPaperPdfOutput {
    let requires_bibliographic_input =
        requires_bibliographic_input(metadata.doi.as_deref(), metadata.title.as_deref());
    ImportPaperPdfOutput {
        status: if requires_bibliographic_input {
            ImportPaperPdfStatus::MetadataRequired
        } else {
            ImportPaperPdfStatus::Imported
        },
        paper_id,
        bucket: bucket.to_string(),
        object_key,
        content_type: content_type.to_string(),
        size_bytes,
        original_filename,
        sha256,
        doi: metadata.doi,
        title: metadata.title,
        authors: metadata.authors,
        publication_year: metadata.publication_year,
        journal: metadata.journal,
        volume: metadata.volume,
        issue: metadata.issue,
        pages: metadata.pages,
        article_number: metadata.article_number,
        requires_bibliographic_input,
        already_imported: false,
    }
}

fn output_from_metadata(
    status: ImportPaperPdfStatus,
    metadata: PaperMetadata,
) -> ImportPaperPdfOutput {
    let requires_bibliographic_input =
        requires_bibliographic_input(metadata.doi.as_deref(), metadata.title.as_deref());
    ImportPaperPdfOutput {
        status: if requires_bibliographic_input {
            ImportPaperPdfStatus::MetadataRequired
        } else {
            status
        },
        paper_id: metadata.id,
        bucket: metadata.bucket,
        object_key: metadata.object_key,
        content_type: metadata.content_type,
        size_bytes: metadata.size_bytes,
        original_filename: metadata.original_filename,
        sha256: metadata.sha256,
        doi: metadata.doi,
        title: metadata.title,
        authors: metadata.authors,
        publication_year: metadata.publication_year,
        journal: metadata.journal,
        volume: metadata.volume,
        issue: metadata.issue,
        pages: metadata.pages,
        article_number: metadata.article_number,
        requires_bibliographic_input,
        already_imported: true,
    }
}

fn requires_bibliographic_input(doi: Option<&str>, title: Option<&str>) -> bool {
    let has_doi = doi.is_some_and(|value| !value.trim().is_empty());
    let has_title = title.is_some_and(|value| !value.trim().is_empty());
    !has_doi && !has_title
}

fn is_valid_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, path::Path, pin::Pin, sync::Mutex};

    use axum::body::Bytes;
    use futures_util::Stream;

    use crate::features::media::service::{
        GetMediaObjectInput, MediaObjectByteStream, MediaServiceError,
    };

    use super::*;

    #[derive(Default)]
    struct FakeObjectStore {
        puts: Mutex<Vec<PutMediaObjectInput>>,
        deletes: Mutex<Vec<DeleteMediaObjectInput>>,
        fail_put: bool,
        fail_delete: bool,
    }

    #[async_trait::async_trait]
    impl MediaObjectStore for FakeObjectStore {
        async fn put_object(&self, input: PutMediaObjectInput) -> Result<(), MediaServiceError> {
            if self.fail_put {
                return Err(MediaServiceError::ObjectStoreFailed);
            }
            self.puts.lock().unwrap().push(input);
            Ok(())
        }

        async fn get_object(
            &self,
            _input: GetMediaObjectInput,
        ) -> Result<MediaObjectByteStream, MediaServiceError> {
            let stream: Pin<Box<dyn Stream<Item = Result<Bytes, MediaServiceError>> + Send>> =
                Box::pin(futures_util::stream::empty());
            Ok(stream)
        }

        async fn delete_object(
            &self,
            input: DeleteMediaObjectInput,
        ) -> Result<(), MediaServiceError> {
            if self.fail_delete {
                return Err(MediaServiceError::ObjectStoreFailed);
            }
            self.deletes.lock().unwrap().push(input);
            Ok(())
        }
    }

    struct FakeExtractor {
        result: Mutex<Option<Result<GrobidPaperMetadata, GrobidError>>>,
        calls: Mutex<usize>,
    }

    impl FakeExtractor {
        fn success(metadata: GrobidPaperMetadata) -> Self {
            Self {
                result: Mutex::new(Some(Ok(metadata))),
                calls: Mutex::new(0),
            }
        }

        fn failure() -> Self {
            Self {
                result: Mutex::new(Some(Err(GrobidError::RequestFailed))),
                calls: Mutex::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl PaperMetadataExtractor for FakeExtractor {
        async fn extract_header(
            &self,
            _pdf_path: &Path,
            _pdf_size_bytes: u64,
        ) -> Result<GrobidPaperMetadata, GrobidError> {
            *self.calls.lock().unwrap() += 1;
            self.result
                .lock()
                .unwrap()
                .take()
                .expect("fake extractor called more than once")
        }
    }

    enum InsertBehavior {
        Inserted,
        Conflict,
    }

    struct FakeRepository {
        finds: Mutex<VecDeque<Option<PaperMetadata>>>,
        insert_behavior: InsertBehavior,
        inserted: Mutex<Vec<OwnedInsertPaperMetadata>>,
    }

    #[derive(Debug)]
    struct OwnedInsertPaperMetadata {
        id: Uuid,
        sha256: String,
        doi: Option<String>,
        title: Option<String>,
        authors: Option<String>,
        publication_year: Option<i32>,
        journal: Option<String>,
        volume: Option<String>,
        issue: Option<String>,
        pages: Option<String>,
        article_number: Option<String>,
    }

    impl FakeRepository {
        fn new(finds: Vec<Option<PaperMetadata>>, insert_behavior: InsertBehavior) -> Self {
            Self {
                finds: Mutex::new(finds.into()),
                insert_behavior,
                inserted: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl PaperImportRepository for FakeRepository {
        async fn find_by_sha256(
            &self,
            _sha256: &str,
        ) -> Result<Option<PaperMetadata>, sqlx::Error> {
            Ok(self.finds.lock().unwrap().pop_front().flatten())
        }

        async fn insert_if_sha256_absent(
            &self,
            metadata: InsertPaperMetadata<'_>,
        ) -> Result<bool, sqlx::Error> {
            self.inserted
                .lock()
                .unwrap()
                .push(OwnedInsertPaperMetadata {
                    id: metadata.id,
                    sha256: metadata.sha256.to_string(),
                    doi: metadata.doi.map(ToString::to_string),
                    title: metadata.title.map(ToString::to_string),
                    authors: metadata.authors.map(ToString::to_string),
                    publication_year: metadata.publication_year,
                    journal: metadata.journal.map(ToString::to_string),
                    volume: metadata.volume.map(ToString::to_string),
                    issue: metadata.issue.map(ToString::to_string),
                    pages: metadata.pages.map(ToString::to_string),
                    article_number: metadata.article_number.map(ToString::to_string),
                });
            Ok(matches!(self.insert_behavior, InsertBehavior::Inserted))
        }
    }

    fn input() -> ImportPaperPdfInput {
        ImportPaperPdfInput {
            bucket: "papers".to_string(),
            uploaded_by: Uuid::new_v4(),
            original_filename: Some("paper.pdf".to_string()),
            content_type: "application/pdf".to_string(),
            file_path: PathBuf::from("/tmp/test-paper.pdf"),
            size_bytes: 1234,
            payload_sha256: "a".repeat(64),
        }
    }

    fn existing_paper() -> PaperMetadata {
        PaperMetadata {
            id: Uuid::new_v4(),
            bucket: "papers".to_string(),
            object_key: "papers/existing/original.pdf".to_string(),
            content_type: "application/pdf".to_string(),
            size_bytes: 1234,
            original_filename: Some("existing.pdf".to_string()),
            sha256: "a".repeat(64),
            doi: Some("10.1/existing".to_string()),
            title: Some("Existing paper".to_string()),
            authors: Some("Doe, Jane".to_string()),
            publication_year: Some(2025),
            journal: Some("Journal".to_string()),
            volume: Some("1".to_string()),
            issue: Some("2".to_string()),
            pages: Some("1-10".to_string()),
            article_number: None,
            uploaded_by: Uuid::new_v4(),
        }
    }

    fn extracted_metadata() -> GrobidPaperMetadata {
        GrobidPaperMetadata {
            doi: Some("10.1234/test".to_string()),
            title: Some("Test paper".to_string()),
            authors: Some("Doe, Jane; Smith, John".to_string()),
            publication_year: Some(2026),
            journal: Some("Test Journal".to_string()),
            volume: Some("12".to_string()),
            issue: Some("3".to_string()),
            pages: Some("10-20".to_string()),
            article_number: Some("e100".to_string()),
        }
    }

    #[tokio::test]
    async fn duplicate_pdf_stops_before_garage_and_grobid() {
        let existing = existing_paper();
        let existing_id = existing.id;
        let repository = FakeRepository::new(vec![Some(existing)], InsertBehavior::Inserted);
        let store = FakeObjectStore::default();
        let extractor = FakeExtractor::success(extracted_metadata());

        let output = PaperImportService::import_pdf_with_dependencies(
            input(),
            &store,
            &repository,
            &extractor,
        )
        .await
        .expect("duplicate should return existing paper");

        assert_eq!(output.status, ImportPaperPdfStatus::AlreadyImported);
        assert_eq!(output.paper_id, existing_id);
        assert!(store.puts.lock().unwrap().is_empty());
        assert!(store.deletes.lock().unwrap().is_empty());
        assert_eq!(*extractor.calls.lock().unwrap(), 0);
        assert!(repository.inserted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn new_pdf_is_stored_extracted_and_inserted_with_metadata() {
        let repository = FakeRepository::new(vec![None], InsertBehavior::Inserted);
        let store = FakeObjectStore::default();
        let extractor = FakeExtractor::success(extracted_metadata());

        let output = PaperImportService::import_pdf_with_dependencies(
            input(),
            &store,
            &repository,
            &extractor,
        )
        .await
        .expect("new paper should import");

        assert_eq!(output.status, ImportPaperPdfStatus::Imported);
        assert_eq!(output.doi.as_deref(), Some("10.1234/test"));
        assert_eq!(output.title.as_deref(), Some("Test paper"));
        assert_eq!(output.article_number.as_deref(), Some("e100"));
        assert_eq!(store.puts.lock().unwrap().len(), 1);
        assert!(store.deletes.lock().unwrap().is_empty());
        assert_eq!(*extractor.calls.lock().unwrap(), 1);

        let inserted = repository.inserted.lock().unwrap();
        assert_eq!(inserted.len(), 1);
        assert_eq!(inserted[0].id, output.paper_id);
        assert_eq!(inserted[0].sha256, "a".repeat(64));
        assert_eq!(inserted[0].doi.as_deref(), Some("10.1234/test"));
        assert_eq!(inserted[0].title.as_deref(), Some("Test paper"));
        assert_eq!(
            inserted[0].authors.as_deref(),
            Some("Doe, Jane; Smith, John")
        );
        assert_eq!(inserted[0].publication_year, Some(2026));
        assert_eq!(inserted[0].journal.as_deref(), Some("Test Journal"));
        assert_eq!(inserted[0].volume.as_deref(), Some("12"));
        assert_eq!(inserted[0].issue.as_deref(), Some("3"));
        assert_eq!(inserted[0].pages.as_deref(), Some("10-20"));
        assert_eq!(inserted[0].article_number.as_deref(), Some("e100"));
    }

    #[tokio::test]
    async fn grobid_failure_rolls_back_garage_and_does_not_insert() {
        let repository = FakeRepository::new(vec![None], InsertBehavior::Inserted);
        let store = FakeObjectStore::default();
        let extractor = FakeExtractor::failure();

        let result = PaperImportService::import_pdf_with_dependencies(
            input(),
            &store,
            &repository,
            &extractor,
        )
        .await;

        assert!(matches!(result, Err(PaperImportServiceError::Grobid(_))));
        assert_eq!(store.puts.lock().unwrap().len(), 1);
        assert_eq!(store.deletes.lock().unwrap().len(), 1);
        assert!(repository.inserted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn concurrent_duplicate_removes_own_object_and_returns_winner() {
        let winner = existing_paper();
        let winner_id = winner.id;
        let repository = FakeRepository::new(vec![None, Some(winner)], InsertBehavior::Conflict);
        let store = FakeObjectStore::default();
        let extractor = FakeExtractor::success(extracted_metadata());

        let output = PaperImportService::import_pdf_with_dependencies(
            input(),
            &store,
            &repository,
            &extractor,
        )
        .await
        .expect("race loser should return winner");

        assert_eq!(output.status, ImportPaperPdfStatus::AlreadyImported);
        assert_eq!(output.paper_id, winner_id);
        assert_eq!(store.puts.lock().unwrap().len(), 1);
        assert_eq!(store.deletes.lock().unwrap().len(), 1);
        assert_eq!(*extractor.calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn garage_put_failure_stops_before_grobid_and_insert() {
        let repository = FakeRepository::new(vec![None], InsertBehavior::Inserted);
        let store = FakeObjectStore {
            fail_put: true,
            ..Default::default()
        };
        let extractor = FakeExtractor::success(extracted_metadata());

        let result = PaperImportService::import_pdf_with_dependencies(
            input(),
            &store,
            &repository,
            &extractor,
        )
        .await;

        assert!(matches!(
            result,
            Err(PaperImportServiceError::ObjectStoreFailed)
        ));
        assert_eq!(*extractor.calls.lock().unwrap(), 0);
        assert!(repository.inserted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn invalid_input_stops_before_all_external_dependencies() {
        let repository = FakeRepository::new(vec![], InsertBehavior::Inserted);
        let store = FakeObjectStore::default();
        let extractor = FakeExtractor::success(extracted_metadata());
        let mut invalid = input();
        invalid.content_type = "text/plain".to_string();

        let result = PaperImportService::import_pdf_with_dependencies(
            invalid,
            &store,
            &repository,
            &extractor,
        )
        .await;

        assert!(matches!(result, Err(PaperImportServiceError::InvalidInput)));
        assert!(store.puts.lock().unwrap().is_empty());
        assert_eq!(*extractor.calls.lock().unwrap(), 0);
        assert!(repository.inserted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn content_type_is_normalized_before_storage() {
        let repository = FakeRepository::new(vec![None], InsertBehavior::Inserted);
        let store = FakeObjectStore::default();
        let extractor = FakeExtractor::success(extracted_metadata());
        let mut mixed_case = input();
        mixed_case.content_type = "Application/PDF".to_string();

        let output = PaperImportService::import_pdf_with_dependencies(
            mixed_case,
            &store,
            &repository,
            &extractor,
        )
        .await
        .expect("MIME type should be case-insensitive");

        assert_eq!(output.content_type, "application/pdf");
        assert_eq!(
            store.puts.lock().unwrap()[0].content_type,
            "application/pdf"
        );
    }

    struct FailingFindRepository;

    #[async_trait::async_trait]
    impl PaperImportRepository for FailingFindRepository {
        async fn find_by_sha256(
            &self,
            _sha256: &str,
        ) -> Result<Option<PaperMetadata>, sqlx::Error> {
            Err(sqlx::Error::RowNotFound)
        }

        async fn insert_if_sha256_absent(
            &self,
            _metadata: InsertPaperMetadata<'_>,
        ) -> Result<bool, sqlx::Error> {
            panic!("insert must not run after initial lookup failure")
        }
    }

    #[tokio::test]
    async fn initial_duplicate_lookup_failure_stops_before_external_dependencies() {
        let repository = FailingFindRepository;
        let store = FakeObjectStore::default();
        let extractor = FakeExtractor::success(extracted_metadata());

        let result = PaperImportService::import_pdf_with_dependencies(
            input(),
            &store,
            &repository,
            &extractor,
        )
        .await;

        assert!(matches!(result, Err(PaperImportServiceError::Database(_))));
        assert!(store.puts.lock().unwrap().is_empty());
        assert!(store.deletes.lock().unwrap().is_empty());
        assert_eq!(*extractor.calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn concurrent_duplicate_without_winner_returns_conflict_resolution_failed() {
        let repository = FakeRepository::new(vec![None, None], InsertBehavior::Conflict);
        let store = FakeObjectStore::default();
        let extractor = FakeExtractor::success(extracted_metadata());

        let result = PaperImportService::import_pdf_with_dependencies(
            input(),
            &store,
            &repository,
            &extractor,
        )
        .await;

        assert!(matches!(
            result,
            Err(PaperImportServiceError::ConflictResolutionFailed)
        ));
        assert_eq!(store.puts.lock().unwrap().len(), 1);
        assert_eq!(store.deletes.lock().unwrap().len(), 1);
        assert_eq!(*extractor.calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn rollback_delete_failure_is_reported_as_object_store_failure() {
        let repository = FakeRepository::new(vec![None], InsertBehavior::Inserted);
        let store = FakeObjectStore {
            fail_delete: true,
            ..Default::default()
        };
        let extractor = FakeExtractor::failure();

        let result = PaperImportService::import_pdf_with_dependencies(
            input(),
            &store,
            &repository,
            &extractor,
        )
        .await;

        assert!(matches!(
            result,
            Err(PaperImportServiceError::ObjectStoreFailed)
        ));
        assert_eq!(store.puts.lock().unwrap().len(), 1);
        assert!(repository.inserted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn oversized_service_input_stops_before_external_dependencies() {
        let repository = FakeRepository::new(vec![], InsertBehavior::Inserted);
        let store = FakeObjectStore::default();
        let extractor = FakeExtractor::success(extracted_metadata());
        let mut oversized = input();
        oversized.size_bytes = i64::MAX as u64 + 1;

        let result = PaperImportService::import_pdf_with_dependencies(
            oversized,
            &store,
            &repository,
            &extractor,
        )
        .await;

        assert!(matches!(result, Err(PaperImportServiceError::InvalidInput)));
        assert!(store.puts.lock().unwrap().is_empty());
        assert!(store.deletes.lock().unwrap().is_empty());
        assert_eq!(*extractor.calls.lock().unwrap(), 0);
        assert!(repository.inserted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn invalid_service_inputs_stop_before_external_dependencies() {
        let mut cases = Vec::new();

        let mut empty_bucket = input();
        empty_bucket.bucket = "   ".to_string();
        cases.push(empty_bucket);

        let mut empty_file = input();
        empty_file.size_bytes = 0;
        cases.push(empty_file);

        let mut short_sha = input();
        short_sha.payload_sha256 = "a".repeat(63);
        cases.push(short_sha);

        let mut non_hex_sha = input();
        non_hex_sha.payload_sha256 = "z".repeat(64);
        cases.push(non_hex_sha);

        for invalid in cases {
            let repository = FakeRepository::new(vec![], InsertBehavior::Inserted);
            let store = FakeObjectStore::default();
            let extractor = FakeExtractor::success(extracted_metadata());

            let result = PaperImportService::import_pdf_with_dependencies(
                invalid,
                &store,
                &repository,
                &extractor,
            )
            .await;

            assert!(matches!(result, Err(PaperImportServiceError::InvalidInput)));
            assert!(store.puts.lock().unwrap().is_empty());
            assert!(store.deletes.lock().unwrap().is_empty());
            assert_eq!(*extractor.calls.lock().unwrap(), 0);
            assert!(repository.inserted.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn service_enforces_100_mib_pdf_limit() {
        let exact_repository = FakeRepository::new(vec![None], InsertBehavior::Inserted);
        let exact_store = FakeObjectStore::default();
        let exact_extractor = FakeExtractor::success(extracted_metadata());
        let mut exact_limit = input();
        exact_limit.size_bytes = PAPER_PDF_FILE_SIZE_LIMIT_BYTES;

        let exact_result = PaperImportService::import_pdf_with_dependencies(
            exact_limit,
            &exact_store,
            &exact_repository,
            &exact_extractor,
        )
        .await;

        assert!(exact_result.is_ok(), "100 MiB must remain valid");
        assert_eq!(exact_store.puts.lock().unwrap().len(), 1);
        assert_eq!(*exact_extractor.calls.lock().unwrap(), 1);

        let over_repository = FakeRepository::new(vec![], InsertBehavior::Inserted);
        let over_store = FakeObjectStore::default();
        let over_extractor = FakeExtractor::success(extracted_metadata());
        let mut over_limit = input();
        over_limit.size_bytes = PAPER_PDF_FILE_SIZE_LIMIT_BYTES + 1;

        let over_result = PaperImportService::import_pdf_with_dependencies(
            over_limit,
            &over_store,
            &over_repository,
            &over_extractor,
        )
        .await;

        assert!(matches!(
            over_result,
            Err(PaperImportServiceError::InvalidInput)
        ));
        assert!(over_store.puts.lock().unwrap().is_empty());
        assert_eq!(*over_extractor.calls.lock().unwrap(), 0);
        assert!(over_repository.inserted.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn new_pdf_without_doi_and_title_is_saved_as_metadata_required() {
        let repository = FakeRepository::new(vec![None], InsertBehavior::Inserted);
        let store = FakeObjectStore::default();
        let mut metadata = extracted_metadata();
        metadata.doi = None;
        metadata.title = None;
        let extractor = FakeExtractor::success(metadata);

        let output = PaperImportService::import_pdf_with_dependencies(
            input(),
            &store,
            &repository,
            &extractor,
        )
        .await
        .expect("a PDF without minimum metadata must still be saved");

        assert_eq!(output.status, ImportPaperPdfStatus::MetadataRequired);
        assert!(output.requires_bibliographic_input);
        assert_eq!(store.puts.lock().unwrap().len(), 1);
        let inserted = repository.inserted.lock().unwrap();
        assert_eq!(inserted.len(), 1);
        assert!(inserted[0].doi.is_none());
        assert!(inserted[0].title.is_none());
    }

    #[tokio::test]
    async fn new_pdf_with_minimum_bibliographic_metadata_is_imported() {
        for (doi, title) in [
            (Some("10.1234/doi-only".to_string()), None),
            (None, Some("Title only".to_string())),
        ] {
            let repository = FakeRepository::new(vec![None], InsertBehavior::Inserted);
            let store = FakeObjectStore::default();
            let mut metadata = extracted_metadata();
            metadata.doi = doi;
            metadata.title = title;
            let extractor = FakeExtractor::success(metadata);

            let output = PaperImportService::import_pdf_with_dependencies(
                input(),
                &store,
                &repository,
                &extractor,
            )
            .await
            .expect("one minimum metadata value is enough");

            assert_eq!(output.status, ImportPaperPdfStatus::Imported);
            assert!(!output.requires_bibliographic_input);
        }
    }

    #[tokio::test]
    async fn duplicate_pdf_without_doi_and_title_requires_metadata() {
        let mut existing = existing_paper();
        let existing_id = existing.id;
        existing.doi = None;
        existing.title = None;
        let repository = FakeRepository::new(vec![Some(existing)], InsertBehavior::Inserted);
        let store = FakeObjectStore::default();
        let extractor = FakeExtractor::success(extracted_metadata());

        let output = PaperImportService::import_pdf_with_dependencies(
            input(),
            &store,
            &repository,
            &extractor,
        )
        .await
        .expect("duplicate without metadata must request completion");

        assert_eq!(output.paper_id, existing_id);
        assert_eq!(output.status, ImportPaperPdfStatus::MetadataRequired);
        assert!(output.requires_bibliographic_input);
        assert!(store.puts.lock().unwrap().is_empty());
        assert_eq!(*extractor.calls.lock().unwrap(), 0);
    }
}

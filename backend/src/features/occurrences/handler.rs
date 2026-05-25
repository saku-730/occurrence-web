use axum::{
    body::Bytes,
    extract::State,
    http::{
        header::{CONTENT_TYPE,COOKIE,},
        HeaderMap,
        StatusCode,
    },
    response::{IntoResponse, Response},
    Json,
};

use crate::{
    features::auth::{
        dto::ErrorResponse,
        service::{AuthService, AuthServiceError},
    },
    state::AppState,
};

use super::{
    dto::CreateOccurrenceResponse,
    service::{
        CreateOccurrenceInput,
        OccurrenceService,
        OccurrenceServiceError,
    },
};

#[derive(Debug)]
pub enum OccurrenceHandlerError {
    InvalidSession, //セッションを持っていないなど
    Database(sqlx::Error),   //posgre側のエラー トークン認証など
    NotImplemented, //
    UnsupportedMediaType, //httpリクエストのbodyがtext/turtle以外など
    EmptyBody, //httpリクエストのbodyが空
    InternalServerError, //サーバー側の処理エラーなど
    InvalidRdf, //フロントから送信されたN-Quadsが壊れている
    RdfStoreError, //
    ForbiddenRdfPredicate,//禁止されている述語を含むRDFを拒否
    ForbiddenRdfGraph,//グラフ名が間違っている場合拒否
    EmptyRdf,//空のデータを拒否
}

impl From<AuthServiceError> for OccurrenceHandlerError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidSession => Self::InvalidSession,
            AuthServiceError::Database(error) => Self::Database(error),
            _ => Self::InvalidSession,
        }
    }
}

impl From<OccurrenceServiceError> for OccurrenceHandlerError {
    fn from(error: OccurrenceServiceError) -> Self {
        match error {
            OccurrenceServiceError::NotImplemented => Self::NotImplemented,
            OccurrenceServiceError::InvalidOccurrenceUri => Self::InternalServerError,
            OccurrenceServiceError::InvalidPredicateUri => Self::InternalServerError,
            OccurrenceServiceError::InvalidUserUri => Self::InternalServerError,
            OccurrenceServiceError::InvalidGraphUri => Self::InternalServerError,
            OccurrenceServiceError::RdfSerializationFailed => Self::InternalServerError,
            OccurrenceServiceError::RdfParseFailed => Self::InvalidRdf,
            OccurrenceServiceError::StoreFailed => Self::RdfStoreError,
            OccurrenceServiceError::FrontendManagedPredicateProvided => Self::ForbiddenRdfPredicate,
            OccurrenceServiceError::ForbiddenRdfGraph => Self::ForbiddenRdfGraph,
            OccurrenceServiceError::EmptyRdf => Self::EmptyRdf,
        }
    }
}

impl IntoResponse for OccurrenceHandlerError {
    fn into_response(self) -> Response {
        match self {
            OccurrenceHandlerError::InvalidSession => {
                let body = ErrorResponse {
                    error: "invalid_session".to_string(),
                    message: "Invalid session".to_string(),
                };

                (StatusCode::UNAUTHORIZED, Json(body)).into_response()
            }
            
            OccurrenceHandlerError::Database(_) => {
                let body = ErrorResponse {
                    error: "internal_server_error".to_string(),
                    message: "Internal server error".to_string(),
                };

                (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
            }
            OccurrenceHandlerError::NotImplemented => {
                let body = ErrorResponse {
                    error: "not_implemented".to_string(),
                    message: "Occurrence creation is not implemented yet".to_string(),
                };

                (StatusCode::NOT_IMPLEMENTED, Json(body)).into_response()
            }
            OccurrenceHandlerError::UnsupportedMediaType => {
                let body = ErrorResponse {
                    error: "unsupported_media_type".to_string(),
                    message: "Unsupported media type".to_string(),
                };

                (StatusCode::UNSUPPORTED_MEDIA_TYPE, Json(body)).into_response()
            }
            OccurrenceHandlerError::EmptyBody => {
                let body = ErrorResponse {
                    error: "empty_body".to_string(),
                    message: "Request body must not be empty".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
            OccurrenceHandlerError::InternalServerError => {
                let body = ErrorResponse {
                    error: "internal_server_error".to_string(),
                    message: "Internal server error".to_string(),
                };

                (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
            }
            OccurrenceHandlerError::RdfStoreError => {
                let body = ErrorResponse {
                    error: "rdf_store_error".to_string(),
                    message: "Failed to save occurrence RDF".to_string(),
                };

                (StatusCode::BAD_GATEWAY, Json(body)).into_response()
            }
            OccurrenceHandlerError::ForbiddenRdfPredicate => {
                let body = ErrorResponse {
                    error: "forbidden_rdf_predicate".to_string(),
                    message: "Frontend RDF must not contain backend-managed predicates".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
            OccurrenceHandlerError::ForbiddenRdfGraph => {
                let body = ErrorResponse {
                    error: "forbidden_rdf_graph".to_string(),
                    message: "Occurrence RDF must use the occurrence graph".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
            Self::InvalidRdf => {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "invalid_rdf".to_string(),
                        message: "Invalid RDF body".to_string(),
                    }),
                )
                    .into_response()
            }
            OccurrenceHandlerError::EmptyRdf => {
                let body = ErrorResponse {
                    error: "empty_rdf".to_string(),
                    message: "Occurrence RDF must contain at least one quad".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
        }
    }
}

pub async fn create_occurrence(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<CreateOccurrenceResponse>), OccurrenceHandlerError> {
    let session_token = extract_session_token(&headers)?;

    let current_user = AuthService::current_user(
        &state.posgre,
        session_token,
    )
    .await?;

    let content_type = ensure_supported_rdf_content_type(&headers)?;
    ensure_non_empty_body(&body)?;

    let input = CreateOccurrenceInput {
        create_user_id: current_user.user_id,
        content_type,
        rdf_body: body.to_vec(),
    };

    let output = OccurrenceService::create_occurrence(
        input,
        state.occurrence_rdf_store.as_ref(),
    )
    .await?;

    let response = CreateOccurrenceResponse {
        occurrence_id: output.occurrence_id.to_string(),
        occurrence_uri: output.occurrence_uri,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, OccurrenceHandlerError> { //トークン取り出し ヘルパー
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(OccurrenceHandlerError::InvalidSession)?
        .to_str()
        .map_err(|_| OccurrenceHandlerError::InvalidSession)?;

    for cookie in cookie_header.split(';') { //session=asdfasdf; user=asdfasdf;...って感じのヘッダー
        let cookie = cookie.trim(); //cookie整形

        if let Some(session_token) = cookie.strip_prefix("session=") {
            if session_token.trim().is_empty() {
                return Err(OccurrenceHandlerError::InvalidSession);
            }

            return Ok(session_token.to_string());
        }
    }

    Err(OccurrenceHandlerError::InvalidSession)
}

fn ensure_supported_rdf_content_type( //content-typeを確認 text/turtle以外はエラー
    headers: &HeaderMap,
) -> Result<String, OccurrenceHandlerError> {
    let content_type = headers
        .get(CONTENT_TYPE)
        .ok_or(OccurrenceHandlerError::UnsupportedMediaType)?
        .to_str()
        .map_err(|_| OccurrenceHandlerError::UnsupportedMediaType)?;

    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    match media_type.as_str() {
        "application/n-quads" => Ok(media_type),
        _ => Err(OccurrenceHandlerError::UnsupportedMediaType),
    }
}

fn ensure_non_empty_body(body: &Bytes) -> Result<(), OccurrenceHandlerError> {
    // HTTP body が完全に空でないことだけを確認する。
    // 空白だけ・コメントだけなど、RDF quad が 0 件になるケースは service 側で EmptyRdf として扱う。
    if body.is_empty() {
        return Err(OccurrenceHandlerError::EmptyBody);
    }

    Ok(())
}
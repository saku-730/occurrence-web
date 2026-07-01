use utoipa::OpenApi;

use crate::features::auth::dto::{
    CompleteRegistrationRequest, CompleteRegistrationResponse, CurrentUserResponse, ErrorResponse,
    LoginRequest, LoginResponse, LogoutResponse, PasswordResetCompleteRequest,
    PasswordResetCompleteResponse, PasswordResetRequest, PasswordResetResponse, RegisterRequest,
    RegisterResponse,
};

use crate::features::media::dto::{UploadMediaRequest, UploadMediaResponse};

use crate::features::occurrences::dto::{
    CreateOccurrenceResponse, DeleteOccurrenceResponse, SearchOccurrenceFilter,
    SearchOccurrenceItem, SearchOccurrencesPage, SearchOccurrencesRequest,
    SearchOccurrencesRequestPage, SearchOccurrencesResponse,
};

// API追加時はhandlerだけでなくこのOpenAPI定義にも登録する。フロントとの契約をここで固定する。
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::features::auth::handler::pre_register,
        crate::features::auth::handler::complete_registration,
        crate::features::auth::handler::request_password_reset,
        crate::features::auth::handler::reset_password,
        crate::features::auth::handler::login,
        crate::features::auth::handler::logout,
        crate::features::auth::handler::me,
        crate::features::occurrences::handler::create_occurrence,
        crate::features::occurrences::handler::search_occurrences,
        crate::features::occurrences::handler::get_occurrence,
        crate::features::occurrences::handler::delete_occurrence,
        crate::features::occurrences::handler::update_occurrence,
        crate::features::media::handler::upload_media,
        crate::features::media::handler::get_media,
    ),
    components(
        schemas(
            RegisterRequest,
            RegisterResponse,
            ErrorResponse,
            CompleteRegistrationRequest,
            CompleteRegistrationResponse,
            PasswordResetRequest,
            PasswordResetResponse,
            PasswordResetCompleteRequest,
            PasswordResetCompleteResponse,
            LoginRequest,
            LoginResponse,
            LogoutResponse,
            CurrentUserResponse,
            CreateOccurrenceResponse,
            DeleteOccurrenceResponse,
            SearchOccurrenceItem,
            SearchOccurrencesPage,
            SearchOccurrencesResponse,
            SearchOccurrencesRequest,
            SearchOccurrenceFilter,
            SearchOccurrencesRequestPage,
            UploadMediaRequest,
            UploadMediaResponse,
        )
    ),
    tags(
        (name = "auth", description = "Authentication endpoints"),
        (name = "occurrences", description = "Occurrence RDF endpoints"),
        (name = "media", description = "Media attachment endpoints")
    )
)]
pub struct ApiDoc;

use utoipa::OpenApi;

use crate::features::auth::dto::{
    AuthModeResponse, CompleteRegistrationRequest, CompleteRegistrationResponse,
    CurrentUserResponse, DemoLoginRequest, ErrorResponse, LoginRequest, LoginResponse,
    LogoutResponse, PasswordResetCompleteRequest, PasswordResetCompleteResponse,
    PasswordResetRequest, PasswordResetResponse, RegisterRequest, RegisterResponse,
    UpdateUserNameRequest,
};

use crate::features::media::dto::{DeleteMediaResponse, UploadMediaRequest, UploadMediaResponse};

use crate::features::occurrence_map::dto::{
    OccurrenceMapFeature, OccurrenceMapFeatureCollection, OccurrenceMapGeometry,
    OccurrenceMapProperties, OccurrenceMapSearchRequest,
};
use crate::features::occurrences::dto::{
    CreateOccurrenceResponse, DarwinCoreTermResponse, DeleteOccurrenceResponse,
    SearchOccurrenceFilter, SearchOccurrenceItem, SearchOccurrencesPage, SearchOccurrencesRequest,
    SearchOccurrencesRequestPage, SearchOccurrencesResponse,
};

// API追加時はhandlerだけでなくこのOpenAPI定義にも登録する。フロントとの契約をここで固定する。
// paper import APIは現在source_handlerへ簡略化中のため、旧dto/handlerのOpenAPI定義は削除している。
// source_handlerへutoipa定義を追加する段階で、新しいpaper APIだけをここへ登録する。
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::features::auth::handler::pre_register,
        crate::features::auth::handler::complete_registration,
        crate::features::auth::handler::request_password_reset,
        crate::features::auth::handler::reset_password,
        crate::features::auth::handler::login,
        crate::features::auth::handler::demo_login,
        crate::features::auth::handler::auth_mode,
        crate::features::auth::handler::logout,
        crate::features::auth::handler::me,
        crate::features::auth::handler::update_user_name,
        crate::features::occurrences::handler::create_occurrence,
        crate::features::occurrences::handler::list_darwin_core_terms,
        crate::features::occurrences::handler::search_occurrences,
        crate::features::occurrences::handler::get_occurrence,
        crate::features::occurrences::handler::delete_occurrence,
        crate::features::occurrences::handler::update_occurrence,
        crate::features::occurrence_map::handler::get_occurrence_map,
        crate::features::occurrence_map::handler::search_occurrence_map,
        crate::features::media::handler::upload_media,
        crate::features::media::handler::get_media,
        crate::features::media::handler::delete_media,
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
            DemoLoginRequest,
            LoginResponse,
            AuthModeResponse,
            LogoutResponse,
            CurrentUserResponse,
            UpdateUserNameRequest,
            CreateOccurrenceResponse,
            DeleteOccurrenceResponse,
            DarwinCoreTermResponse,
            SearchOccurrenceItem,
            SearchOccurrencesPage,
            SearchOccurrencesResponse,
            SearchOccurrencesRequest,
            SearchOccurrenceFilter,
            SearchOccurrencesRequestPage,
            OccurrenceMapSearchRequest,
            OccurrenceMapFeatureCollection,
            OccurrenceMapFeature,
            OccurrenceMapGeometry,
            OccurrenceMapProperties,
            UploadMediaRequest,
            UploadMediaResponse,
            DeleteMediaResponse,
        )
    ),
    tags(
        (name = "auth", description = "Authentication endpoints"),
        (name = "occurrences", description = "Occurrence RDF endpoints"),
        (name = "media", description = "Media attachment endpoints"),
        (name = "paper-import", description = "Paper PDF import endpoints"),
        (name = "vocabularies", description = "Read-only RDF vocabulary endpoints")
    )
)]
pub struct ApiDoc;

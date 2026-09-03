# テスト一覧

## user register

### app

- [x] `/` にアクセスすると `200 OK` が返り、レスポンスボディが `Occurrence App Backend` である。`index_route_returns_backend_name`

- [x] `/health` にアクセスすると `200 OK` が返り、レスポンスボディが `ok` である。`health_route_returns_ok`

- [x] `POST /auth/pre_register` に正常な email JSON を送ると `201 Created` が返る。`register_route_returns`

- [x] `POST /auth/pre_register` に JSON body なしで送ると client error が返る。`register_route_rejects_missing_json_body`

- [x] `POST /auth/pre_register` に正常な email JSON を送ると `201 Created` が返り、レスポンスJSONが `temporary registration accepted` と正規化済み email を含む。`register_route_returns_created_json_for_valid_email`

- [x] `POST /auth/pre_register` に不正な email を送ると `400 Bad Request` が返り、エラーレスポンスが `invalid_email` / `Invalid email` になる。`register_route_returns_bad_request_for_invalid_email`

- [x] `/openapi.json` にアクセスすると `200 OK` が返り、OpenAPI JSON に `/auth/pre_register`、`RegisterRequest`、`RegisterResponse`、`ErrorResponse` が含まれる。`openapi_json_returns_auth_register_spec`

- [x] `POST /auth/pre_register` に正常な email を送ると、route 経由で `pending_registrations` に1件作成される。`pre_register_route_creates_pending_registration`

- [x] `POST /auth/pre_register` に不正な email を送ると `400 Bad Request` が返り、`pending_registrations` には作成されない。`pre_register_route_rejects_invalid_email_and_does_not_create_pending_registration`

- [x] `/openapi.json` の `/auth/pre_register` の `post.responses` に `201`、`400`、`500` が含まれる。`openapi_json_includes_pre_register_response_statuses`
- [x]  `POST /auth/pre_register` に正常な emailを送ると、トークンが作られhashがpostgresSQLのpending_registrationに保存される。`pre_register_route_creates_token_hash_for_valid_email`
- [x]  `/auth/pre_register`に正常なemailが送られると、そのemail宛に登録用urlを本文に含むメールが送信される。mailpitで確認
- [x]  `/auth/pre_register`に正常なemailが送られると、そのemail宛に登録用urlを本文に含むメールが送信される。Gmailで確認
- [x]  `POST /auth/complete_registration` に JSON body なしで送ると client error が返る`complete_registration_route_rejects_missing_json_body`
- [x] `POST /auth/complete_registration` に有効な token / user_name / password を送ると201 Created が返り、users にユーザーが作成される
- [x] `POST /auth/complete_registration` に登録済みのemailを送ると拒否する

### service

- [x] 正常な email を渡すと、`pre_register` が成功し、レスポンスに正規化済み email と `temporary registration accepted` が入り、`pending_registrations` に1件作成される。`pre_register_accepts_valid_email_and_creates_pending_registration`

- [x] 前後空白と大文字を含む email を渡すと、trim と lowercase が行われ、正規化済み email で `pending_registrations` に1件作成される。`pre_register_trims_and_lowercases_email_and_creates_pending_registration`

- [x] 空白だけの email を渡すと、`AuthServiceError::InvalidEmail` が返る。`pre_register_rejects_empty_email`

- [x] `@` を含まない email を渡すと、`AuthServiceError::InvalidEmail` が返る。`pre_register_rejects_email_without_at`

- [x] local part がない email、つまり `@example.com` を渡すと、`AuthServiceError::InvalidEmail` が返る。`pre_register_rejects_email_without_local_part`

- [x] domain part がない email、つまり `test@` を渡すと、`AuthServiceError::InvalidEmail` が返る。`pre_register_rejects_email_without_domain_part`

- [x] `@` が複数ある email、つまり `test@@example.com` を渡すと、`AuthServiceError::InvalidEmail` が返る。`pre_register_rejects_email_with_multiple_at_marks`

- [x] 正常な email で `pre_register` すると、DBに保存された `token_hash` が64文字で、全て16進数文字である。`pre_register_stores_token_hash`

- [x] 不正な email を渡すと、`AuthServiceError::InvalidEmail` が返り、`pending_registrations` には作成されない。`pre_register_rejects_invalid_email_and_does_not_create_pending_registration`
- [x] AuthService::pre_register に正常な email を渡すと、登録完了URLを本文に含む MailMessage が作成される`pre_register_creates_registration_completion_email`
- [x] complete_registration は空 token を拒否する`complete_registration_rejects_empty_token`
- [x] 空パスワードを拒否`complete_registration_rejects_empty_password`
- [x] パスワードが空白だけを拒否`complete_registration_rejects_blank_password`
- [x] パスワードが8文字未満なら拒否する`complete_registration_rejects_password_shorter_than_8_characters`
- [x] パスワードが128文字を超えるなら拒否する`complete_registration_rejects_password_longer_than_128_characters`
- [x] ユーザー名が空だと拒否`complete_registration_rejects_empty_user_name`
- [x] ユーザー名が空白だと拒否`complete_registration_rejects_blank_user_name`
- [x] complete_registration は存在しない token を拒否する`complete_registration_rejects_unknown_token`
- [x] トークンでpendingテーブルからユーザー探して、作成・本登録。`complete_registration_creates_user_for_valid_token`
- [x] 本登録できたら、pending_registratiosのcompleted_atを更新する`complete_registration_marks_pending_registration_as_completed`
- [x] 使用済みtokenでは、本登録ができない。`complete_registration_rejects_already_completed_token`
- [x] 本登録で期限切れトークンを拒否
- [x] pending_registrations に有効な token があっても、その email の user がすでに users に存在するなら、本登録は失敗する`complete_registration_rejects_email_already_registered`
- [x] トランザクション処理テスト。ユーザー登録を途中でしくじったら、completed_atをロールバック。

### repository

- [x] 正常な形式で `pending_registrations` に `email`、`token_hash`、`expires_at` を INSERT できる。保存後、`email` と `token_hash` が一致し、`completed_at` は `NULL`、`expires_at` は現在時刻より未来である。`create_pending_registration_inserts_row`

- [x] 同じ `token_hash` で2回 INSERT しようとすると、1回目は成功し、2回目は `UNIQUE` 制約により失敗する。`create_pending_registration_rejects_duplicate_token_hash`

### mail

- [x] `POST /auth/pre_register` に正常な email を送ると、登録完了URLを含むメール文面が作成される`builds_registration_completion_email_with_completion_url`
- [x]  send_mail が Config の SMTP 設定を使って Mailpit にメールを送信できる `send_mail_sends_message_using_smtp_config`

### other

- [x] `config.rs` の `Config::from_env` が、`APP_HOST`、`APP_PORT`、`APP_BASE_URL`、`DATABASE_URL` を正しく読むことを確認する
- [x] Config::from_env が SMTP_HOST、SMTP_PORT、SMTP_USERNAME、SMTP_PASSWORD、SMTP_TLS、MAIL_FROM を正しく読むことを確認する `from_env_reads_app_host_port_base_url_and_database_url`

## Password reset

### app

- [x] `POST /auth/request_password_reset` に登録済みユーザーの正しい email を送ると、AuthService経由でパスワードリセット用 token hash が `password_reset_tokens` に保存され、リセットリンクを含む案内メールが送信され、`200 OK` が返る。`request_password_reset_route_sends_reset_mail_for_registered_email`
- [x] `POST /auth/request_password_reset` に未登録 email を送っても、登録有無を推測されないように `200 OK` の成功風レスポンスを返し、パスワードリセット用 token hash は作成されず、メールも送信されない。`request_password_reset_route_returns_success_like_response_for_unregistered_email`
- [x] `POST /auth/request_password_reset` を本番SMTP設定でapp経由実行し、仮ユーザー `test@gmail.com` 宛に`https://bio-database.net` のリセットリンクを含むパスワードリセット案内メールを実送信できる（ignored）。`request_password_reset_route_sends_real_email_to_gmail_for_temporary_user`
- [x] `POST /auth/reset_password` に正常な token と新しいpasswordを送ると、app経由で対象ユーザーの `users.password_hash` が更新される。`reset_password_route_updates_password_for_valid_token`

### service

- [x] 登録済みユーザーの正しい email を `AuthService::request_password_reset` に渡すと、パスワードリセット用 token hash が `password_reset_tokens` に保存され、リセットリンクを含む `MailMessage` が作成される。`request_password_reset_creates_reset_email_for_registered_email`
- [x] 正常なパスワードリセット token と新しいpasswordを `AuthService::reset_password` に渡すと、tokenから対象ユーザーを特定して `users.password_hash` が更新される。`reset_password_updates_password_for_valid_token`
- [x] `AuthService::reset_password` に8文字未満または128文字超のpasswordを渡すと、`AuthServiceError::InvalidPassword` で拒否される。`reset_password_rejects_password_outside_8_to_128_characters`
- [x] `AuthService::reset_password` に正常ではない token を渡すと、`AuthServiceError::InvalidToken` で拒否され、`users.password_hash` は更新されない。`reset_password_rejects_invalid_token_and_does_not_update_password`
- [x] `AuthService::reset_password` で使用済み token を再利用しようとすると、`AuthServiceError::InvalidToken` で拒否され、`users.password_hash` は再更新されない。`reset_password_rejects_used_token_and_does_not_update_password_again`
- [x] `AuthService::reset_password` で期限切れ token を使おうとすると、`AuthServiceError::InvalidToken` で拒否され、`users.password_hash` は更新されない。`reset_password_rejects_expired_token_and_does_not_update_password`
- [x] `AuthService::reset_password` が正常完了したら、対象ユーザーの既存セッションが無効化される。`reset_password_revokes_existing_sessions_for_user`

## Media attachments

### app

- [x] `POST /media` に有効 session と有効な multipart file を送ると、Garage/S3互換 object storageへの書き込みとPostgreSQL `media_objects`へのmetadata保存が行われ、`201 Created` とmetadata JSONが返る。`upload_media_route_saves_object_and_metadata_to_postgresql`
- [x] 未ログインで `POST /media` に有効な multipart file を送ると `401 Unauthorized` を返し、object storageには書き込まれない。`upload_media_route_without_session_returns_unauthorized_and_does_not_write_object`
- [x] `POST /media` に全体上限 1000MB を超える `Content-Length` の添付データを送ると、`413 Payload Too Large` を返し、object storage には書き込まれない。`upload_media_route_rejects_payload_larger_than_global_limit_and_does_not_write_object`
- [x] `POST /media` はAxum既定2MBを超える有効ファイルを受け付け、chunk経由でobject storageへ保存する。`upload_media_route_accepts_body_larger_than_axum_default_limit`
- [x] `POST /media` の一時ファイルはobject storage保存の成功・失敗にかかわらず削除される。`upload_media_route_removes_temporary_file_after_upload`
- [x] ログイン済みのmedia所有者が `GET /media/{media_id}` を呼ぶと、app経由で `MediaService::get_media` が使われ、保存MIME・Content-Length・ファイルstreamを含む `200 OK` が返る。`get_media_route_returns_object_stream_for_owner`
- [x] media所有者ではないユーザーのsessionで `GET /media/{media_id}` を呼ぶと、ファイルを返さず `404 Not Found` になる。`get_media_route_returns_not_found_for_non_owner`
- [x] public occurrence RDFからmedia URIが参照されている場合、未ログインで `GET /media/{media_id}` を呼んでも保存MIME・Content-Length・ファイルstreamを含む `200 OK` が返る。`get_media_route_allows_anonymous_access_when_linked_from_public_occurrence`
- [x] public occurrence RDFからmedia URIが参照されている場合、media所有者とは異なるログイン済みユーザーが `GET /media/{media_id}` を呼んでも `200 OK` でファイルを取得できる。`get_media_route_allows_logged_in_non_owner_when_linked_from_public_occurrence`
- [x] private occurrence RDFからmedia URIが参照されている場合、未ログインで `GET /media/{media_id}` を呼ぶとファイルを返さず `404 Not Found` になる。`get_media_route_hides_private_occurrence_media_from_anonymous_user`
- [x] private occurrence RDFからmedia URIが参照されている場合、media所有者とは異なるログイン済みユーザーが `GET /media/{media_id}` を呼ぶとファイルを返さず `404 Not Found` になる。`get_media_route_hides_private_occurrence_media_from_logged_in_non_owner`
- [x] media所有者の有効sessionで `DELETE /media/{media_id}` を呼ぶと、app経由で `MediaService::delete_media` が使われ、Garage objectとPostgreSQL metadataが削除され `200 OK` と `{"deleted":true}` が返る。`delete_media_route_deletes_owned_media_object_and_metadata`
- [x] media所有者とは異なるユーザーの有効sessionで `DELETE /media/{media_id}` を呼ぶと `404 Not Found` となり、Garage objectもPostgreSQL metadataも削除されない。`delete_media_route_rejects_non_owner_and_preserves_object_and_metadata`
- [x] 未ログインで `DELETE /media/{media_id}` を呼ぶと `401 Unauthorized` となり、Garage objectもPostgreSQL metadataも削除されない。`delete_media_route_requires_login_and_preserves_object_and_metadata`
- [x] ログイン済みユーザーが存在しない `media_id` を指定して `DELETE /media/{media_id}` を呼ぶと `404 Not Found` となり、Garage削除は呼ばれない。`delete_media_route_returns_not_found_for_missing_media`
- [x] ログイン済みユーザーが不正なUUIDを指定して `DELETE /media/{media_id}` を呼ぶと `400 Bad Request` となり、Garage削除は呼ばれない。`delete_media_route_returns_bad_request_for_invalid_media_id`
- [x] media所有者が `DELETE /media/{media_id}` を呼んでも、OccurrenceRdfStoreにmedia URIへの参照が1件以上残っていれば `409 Conflict` となり、Garage objectとPostgreSQL metadataは削除されない。`delete_media_route_returns_conflict_when_occurrence_reference_remains`
- [x] media所有者が `DELETE /media/{media_id}` を呼んだ際にGarage object削除が失敗すると `502 Bad Gateway` となり、PostgreSQL metadataは削除されない。`delete_media_route_returns_bad_gateway_when_garage_delete_fails`

### service

- [x] `MediaService::upload_media` に有効な添付データを渡すと、Garage/S3互換 object storage に object が書き込まれ、`media_id` と `media_uri`、`object_key`、`content_type`、`size_bytes` を含む結果が返る。`upload_media_writes_attachment_object_and_returns_media_metadata`
- [x] `MediaService::upload_media` に spec で許可していない `content_type` を渡すと `MediaServiceError::InvalidInput` で拒否され、object storage には書き込まれない。`upload_media_rejects_unsupported_content_type_and_does_not_write_object`
- [x] 許可MIMEを申告していても実データのmagic bytesが別形式なら、`MediaService::upload_media` は `InvalidInput` で拒否しobject storageへ書き込まない。`upload_media_rejects_content_when_detected_mime_does_not_match_declared_mime`
- [x] 実データと申告MIMEが一致していても、元ファイル名の拡張子がMIMEと一致しなければ `MediaService::upload_media` は `InvalidInput` で拒否しobject storageへ書き込まない。`upload_media_rejects_filename_extension_that_does_not_match_mime`
- [x] `MediaService` の添付データサイズ判定は1000MBを許可し、1001MBを `MediaServiceError::PayloadTooLarge` で拒否する。`media_size_validation_accepts_1000_mb_and_rejects_1001_mb`
- [x] `MediaService::upload_media` に有効な添付データを渡すと、返された `media_id` を主キーとして `media_objects` にGarage保存先、MIME type、サイズ、元ファイル名、登録ユーザーが保存される。`upload_media_saves_metadata_to_postgresql`
- [x] 同じユーザーが同じSHA-256のファイルを再アップロードすると、Garageへ再保存せず既存 `media_id` とmetadataを返し、`media_objects`も1件のままになる。`upload_media_reuses_existing_media_for_same_user_and_sha256`
- [x] Garageへのobject保存成功後にPostgreSQL `media_objects` のINSERTが失敗すると、同じGarage objectを削除して補償し、`MediaServiceError::Database`を返す。`upload_media_deletes_garage_object_when_postgresql_metadata_save_fails`
- [x] `MediaService::get_media` に存在する `media_id` を渡すと、PostgreSQLのmetadataとobject storageのファイルstreamを取得できる。`get_media_returns_metadata_and_object_stream_for_existing_media`
- [x] `MediaService::delete_media` に存在する `media_id` を渡すと、PostgreSQL metadataに記録されたGarage objectを削除し、`media_objects`の行も削除して `deleted=true` を返す。`delete_media_removes_object_and_metadata_by_id`
- [x] `MediaService::delete_media` でGarage object削除後にPostgreSQL metadata削除が失敗すると、同じGarage objectを元のbytes・MIMEで再保存して巻き戻し、metadataを保持したまま `MediaServiceError::Database` を返す。`delete_media_restores_garage_object_when_postgresql_delete_fails`

### config

- [x] `Config::from_env` は `S3_BUCKET` を読み込み、media uploadで使うGarage bucket設定として保持する。`from_env_reads_s3_bucket`

## Session, Login/Logout

### app

- [x] `POST /auth/login`に JSON body なしでおくると client error`login_route_rejects_missing_json_body`
- [x] `POST /auth/login` に登録済み email と正しい password を送ると 200 OK が返る`login_route_returns_ok_for_registered_user_with_correct_password`
- [x] 存在しない email で `POST /auth/login` しても 401 Unauthorized``
- [x] 間違った、パスワードで`POST /auth/login` しても401
- [x] `POST /auth/login` に正常リクエストでCookiセッション発行される。`login_route_sets_session_cookie_for_registered_user`
- [x] `COOKIE_SECURE=true` のとき `POST /auth/login` の session cookie に `Secure` が付く`login_route_sets_secure_session_cookie_when_cookie_secure_enabled`
- [x] `POST /auth/logout`に正常リクエストでログアウト`logout_route_revokes_session_and_clears_cookie`
- [x] `POST /auth/logout`にsession cookie なしで送ると401`logout_route_returns_unauthorized_without_session_cookie`
- [x] `GET /auth/me`に正常 session cookieでユーザー情報取得``
- [x] `GET /auth/me`に session cookieなしで送ると401`me_route_returns_unauthorized_without_session_cookie`
- [x] ログアウト済み session Cookie で `GET /auth/me` にアクセスすると 401 Unauthorized`me_route_returns_unauthorized_for_revoked_session_cookie`
- [x] 期限切れ session Cookie で `GET /auth/me` にアクセスすると 401 Unauthorized`me_route_returns_unauthorized_for_expired_session_cookie`

### service

- [x] 登録済みユーザーが正しい password で login できる`login_accepts_registered_user_with_correct_password`
- [x] 間違ったパスワードを拒否する`login_rejects_registered_user_with_wrong_password`
- [x] 存在しないメールアドレスを拒否する`login_rejects_unknown_email`
- [x] ログインでセッションが作成される`login_creates_session_for_registered_user_with_correct_password`
- [x] ログアウトしたら、posgre sessionsテーブルのrevokedが更新されてセッションが無効になる。`me_route_returns_current_user_for_valid_session_cookie`
- [x] セッショントークンで現在のユーザーを参照できる。`current_user_returns_user_for_valid_session`

## Occurrence data register

### app

- [x] `POST /occurrences`はCookieがなければ401`create_occurrence_route_requires_login`
- [x] `POST /occurrences`はCookieが無効なら401`create_occurrence_route_returns_unauthorized_for_invalid_session_cookie`
- [x] `POST /occurrences`はCookieが有効なら501`create_occurrence_route_with_valid_session_returns_not_implemented` 未実装だから一旦
- [x] `POST /occurrences`はhttpリクエストのbodyがN-Quads以外は拒否415`create_occurrence_route_rejects_unsupported_content_type`
- [x] `POST /occurrences`はhttpリクエストのbodyが空なら400`create_occurrence_route_rejects_empty_body`
- [x] `POST /occurrences`に有効なユーザーで有効リクエストしたときに201created response`create_occurrence_route_with_valid_session_returns_created`
- [x] `POST /occurrences`に有効 session と正しい N-Quads を送ると、route 経由で保存用 N-Quads が OccurrenceRdfStore に渡される`create_occurrence_route_with_valid_session_saves_nquads_to_store`
- [x] `POST /occurrences`に有効 session と壊れた N-Quads を送ると、400 Bad Request を返し、OccurrenceRdfStore には保存されない。`create_occurrence_route_with_invalid_nquads_returns_bad_request_and_does_not_save`
- [x] `POST /occurrences` のN-Quadsにログインユーザーとは別のユーザーが所有するmedia URIが含まれる場合、`403 Forbidden`で拒否しOccurrenceRdfStoreへ保存しない。`create_occurrence_route_rejects_media_owned_by_another_user_and_does_not_save`
- [x] `POST /occurrences`にaccessRightsのリテラル、不正URI、複数指定を送ると400 Bad Requestを返し、OccurrenceRdfStoreには保存されない`create_occurrence_route_rejects_invalid_access_rights_and_does_not_save`
- [x] `POST /occurrences`に有効 session と正しい N-Quads を送ったが、OccurrenceRdfStore の保存処理が失敗した場合、502 Bad Gateway`create_occurrence_route_when_rdf_store_fails_returns_bad_gateway`
- [x] `POST /occurrences`にfrontend が backend 管理 predicate を送ってきたら拒否する`create_occurrence_route_rejects_frontend_creator_and_does_not_save`
- [x] `POST /occurrences`にcreatedまたはmodifiedが最初から入っていたら400 Bad Requestを返し、OccurrenceRdfStoreには保存されない`create_occurrence_route_rejects_frontend_created_or_modified_and_does_not_save`
- [x] N-Quadsのグラフ名が`<https://bio-database.net/graphs/occurrences>`以外拒否で400 `create_occurrence_route_rejects_non_occurrence_graph_and_does_not_save`
- [x] `POST /occurrences`にgraph nameなしN-Quadsを送ると400 Bad Requestを返し、OccurrenceRdfStoreには保存されない`create_occurrence_route_rejects_missing_graph_name_and_does_not_save`
- [x] `POST /occurrences`にsubjectがURIまたは複数blank nodeのN-Quadsを送ると400 Bad Requestを返し、OccurrenceRdfStoreには保存されない`create_occurrence_route_rejects_invalid_blank_node_subject_and_does_not_save`
- [x] `POST /occurrences`にobject blank nodeを含むN-Quadsを送ると400 Bad Requestを返し、OccurrenceRdfStoreには保存されない`create_occurrence_route_rejects_object_blank_node_and_does_not_save`
- [x] `POST /occurrences`に空のデータが送信されたときに、データがつくられない。creatorだけつくられることがない`create_occurrence_route_rejects_empty_rdf_and_does_not_save`
- [x] `POST /occurrences` に有効 session と正しい N-Quads を送ると、実 Fuseki に保存され、SPARQL ASK で取得できる。

### service

- [x] フロントエンドから送られたN-Quadsのblank node subjectをバックエンドが発行したオカレンスuuidに差し替え`replace_all_subjects_with_occurrence_uri_replaces_blank_node_subjects`
- [x] フロントから送られた、N-Quadsにcreate_user_idを付加`add_create_user_id_quad_adds_creator_resource_in_occurrence_graph`
- [x] フロントから送られたN-Quadsをパースしてuser_id追加して、再度シリアライズできるserialize_quads_as_nquads_outputs_named_graph_quads`
- [x] フロントから送られた、N-Quadsを組み立てできる。`build_occurrence_nquads_replaces_subject_and_adds_creator`
- [x] Identification / Event / Location対象述語があると、各`/1`中間ノード・`has*`接続・規定`rdf:type`を生成する`build_occurrence_nquads_creates_intermediate_nodes_for_routed_predicates`
- [x] objectKindがliteralの述語にIRI目的語が来たら`iriEquivalent`へ、objectKindがIRIの述語にリテラル目的語が来たら`literalEquivalent`へ変換する`create_occurrence_converts_predicate_by_object_kind_equivalent`
- [x] objectKindがmixedまたは未定義の述語は目的語型に関係なく変換しない`create_occurrence_keeps_predicate_when_object_kind_is_mixed_or_missing`
- [x] 仕様に列挙された全Identification / Event / Location述語が正しいtargetへ分類される`occurrence_target_routes_all_configured_predicates`
- [x] 対象述語がない種別の空中間ノードを生成せず、unknown predicateとfrontendの`rdf:type`をOccurrence直下に保持する`build_occurrence_nquads_omits_unused_nodes_and_keeps_unrouted_predicates_on_occurrence`
- [x] frontendから`hasIdentification` / `hasEvent` / `hasLocation`が送られたら登録を拒否する`build_occurrence_nquads_rejects_frontend_intermediate_link_predicates`
- [x] UUIDを発行してN-Quadsを組み立てできる。``
- [x] 現在時刻をもとに、フロントからおくられたN-Quadsにcreatedを付加`add_created_quad_adds_created_datetime_in_occurrence_graph`
- [x] 現在時刻をもとに、フロントからおくられたN-Quadsにmodifiedを付加`add_modified_quad_adds_modified_datetime_in_occurrence_graph`
- [x] フロントからaccessRightsが送られていなかったらpublicのaccessRightsを付加`add_default_access_rights_quad_if_missing_adds_public_access_rights`
- [x] フロントからaccessRightsが送られていたらaccessRightsを追加しない`add_default_access_rights_quad_if_missing_keeps_frontend_access_rights`
- [x] フロントからaccessRightsがリテラルで送られていたらデータ登録を拒否`build_occurrence_nquads_rejects_literal_access_rights`
- [x] フロントからaccessRightsが許可URI以外で送られていたらデータ登録を拒否`build_occurrence_nquads_rejects_unknown_access_rights_uri`
- [x] フロントからaccessRightsが複数送られていたらデータ登録を拒否`build_occurrence_nquads_rejects_multiple_access_rights`
- [x] フロントからlicenseにCreative Commons以外のURIが送られていたらデータ登録を拒否`build_occurrence_nquads_rejects_non_creative_commons_license_uri`
- [x] フロントからbackend管理述語 creator / created / modified のいずれかが送られていたらデータ登録を拒否`build_occurrence_nquads_rejects_frontend_backend_managed_predicates`
- [x] フロントからsubjectがblank nodeではなくURIで送られていたらデータ登録を拒否`build_occurrence_nquads_rejects_named_node_subject`
- [x] フロントから複数のblank node subjectが送られていたらデータ登録を拒否`build_occurrence_nquads_rejects_multiple_blank_node_subjects`
- [x] フロントからobject blank nodeが送られていたらデータ登録を拒否`build_occurrence_nquads_rejects_object_blank_node`
- [x] フロントからvalidなaccessRights public/privateが送られていたらbuild後も保持される`build_occurrence_nquads_keeps_valid_access_rights_values`

### repository

### other

- [x] fuseki.rsがfusekiにrdfを保存できる`fuseki_client_save_nquads_inserts_data_into_fuseki`
- [x] fuseki.rsがfusekiに保存したrdfをoccurrence_idで呼び出しできる`fuseki_client_get_occurrence_nquads_returns_only_requested_occurrence`

## Occurrence data update

### app

- [x] `PUT /occurrences/{occurrence_id}`に有効 session と正しい N-Quads を送ると、既存creator/createdを維持して同じoccurrence URIのRDFを更新できる`update_occurrence_route_with_valid_session_updates_existing_occurrence`
- [x] 非ログインユーザーが`PUT /occurrences/{occurrence_id}`で更新しようとすると401になり、RDFは置換されない`update_occurrence_route_requires_login_and_does_not_update`
- [x] occurrence作成者が `PUT /occurrences/{occurrence_id}` のN-Quadsへ別ユーザー所有のmedia URIを追加しようとすると、`403 Forbidden`で拒否され既存RDFは置換されない。`update_occurrence_route_rejects_media_owned_by_another_user_and_does_not_update`
- [x] ログイン済みeditorが他人のoccurrenceを`PUT /occurrences/{occurrence_id}`で更新しようとすると404になり、RDFは置換されない`update_occurrence_route_hides_other_users_occurrence_from_editor_and_does_not_update`

### service

- [x] `OccurrenceService::update_occurrence` は既存creator/createdを維持し、modifiedを更新して、同じoccurrence URIで置換保存する`update_occurrence_preserves_creator_and_created_updates_modified_and_replaces_same_occurrence_uri`
- [x] `OccurrenceService::update_occurrence` は更新入力を再正規化し、対象述語を`/1`中間ノードへ保存する`update_occurrence_rebuilds_intermediate_nodes_for_routed_predicates`


## Occurrence data delete

### app

- [x] `DELETE /occurrences/{occurrence_id}`に有効 session を送ると、OccurrenceService経由で既存occurrence RDFを削除し`{"deleted":true}`を返す`delete_occurrence_route_with_valid_session_deletes_existing_occurrence`
- [x] 非ログインユーザーが`DELETE /occurrences/{occurrence_id}`で削除しようとすると401になり、RDFは削除されない`delete_occurrence_route_requires_login_and_does_not_delete`
- [x] ログイン済みユーザーが存在しないoccurrence_idを`DELETE /occurrences/{occurrence_id}`で削除しようとすると404になる`delete_occurrence_route_returns_not_found_for_missing_occurrence`
- [x] `DELETE /occurrences/{occurrence_id}`でOccurrenceRdfStoreの削除処理が失敗したら502 Bad Gatewayを返す`delete_occurrence_route_when_rdf_store_delete_fails_returns_bad_gateway`
- [x] ログイン済みeditorが他人のoccurrenceを`DELETE /occurrences/{occurrence_id}`で削除しようとすると404になり、RDFは削除されない`delete_occurrence_route_hides_other_users_occurrence_from_editor_and_does_not_delete`
- [x] `DELETE /occurrences/{occurrence_id}`に有効 session を送ると、実 Fuseki に保存済みの occurrence RDF が削除される（ignored）`delete_occurrence_route_deletes_existing_occurrence_from_real_fuseki`

### service

- [x] `OccurrenceService::delete_occurrence` はoccurrence_idからoccurrence URIを組み立て、そのURIのRDFを削除する`delete_occurrence_deletes_existing_occurrence_nquads_by_occurrence_uri`
- [x] Fuseki削除SPARQLはOccurrence本体と接続されたIdentification / Event / Locationのquadを削除する`build_delete_occurrence_update_includes_intermediate_nodes`

## Occurrence data detail

### app

- [x] `GET /occurrences/{occurrence_id}`指定された occurrence_id から occurrence_uri を組み立てる。OccurrenceRdfStore からその occurrence_uri の N-Quads を取得する。存在すれば 200 OK / application/n-quads で返す`get_occurrence_route_returns_nquads_for_existing_occurrence`
- [x] 非ログインユーザーはpublic occurrenceを閲覧できる`get_occurrence_route_allows_anonymous_user_to_view_public_occurrence`
- [x] 非ログインユーザーはprivate occurrenceを閲覧できず404 Not Foundを返す`get_occurrence_route_hides_private_occurrence_from_anonymous_user`
- [x] editorは自分のprivate occurrenceを閲覧できる`get_occurrence_route_allows_editor_to_view_own_private_occurrence`
- [x] editorは他人のprivate occurrenceを閲覧できず404 Not Foundを返す`get_occurrence_route_hides_other_users_private_occurrence_from_editor`
- [ ] adminは他人のprivate occurrenceを含む全occurrenceを閲覧できる`get_occurrence_route_allows_admin_to_view_other_users_private_occurrence`
- [x] `GET /occurrences/{occurrence_id}`で存在しないoccurrence_idのとき404`get_occurrence_route_returns_not_found_for_missing_occurrence`
- [x] `GET /occurrences/{occurrence_id}`でoccurrence_idがUUIDではないとき400 Bad Requestを返す`get_occurrence_route_returns_bad_request_for_invalid_occurrence_id`
- [x] `GET /occurrences/{occurrence_id}`でFusekiへの問い合わせ失敗で502`get_occurrence_route_when_rdf_store_fails_returns_bad_gateway`
- [x] `GET /occurrences/{occurrence_id}`で実Fusekiからpublic occurrenceのN-Quadsを取得できる（ignored）`get_occurrence_route_returns_nquads_from_real_fuseki`

### Service

- [x] `OccurrenceService::get_occurrence` は指定された occurrence_id から occurrence_uri を組み立て、OccurrenceRdfStore から該当 N-Quads を取得できる`get_occurrence_returns_nquads_for_requested_occurrence_uri`
- [x] `OccurrenceService::get_occurrence` はOccurrenceRdfStoreがNoneを返したらOk(None)を返す`get_occurrence_returns_none_when_store_returns_none`
- [x] `OccurrenceService::get_occurrence` はOccurrenceRdfStoreがStoreFailedを返したらそのエラーを伝播する`get_occurrence_propagates_store_failed_error`
- [x] Fuseki詳細取得CONSTRUCTはOccurrence本体と接続されたIdentification / Event / Locationを平坦化せず取得する`build_get_occurrence_query_includes_intermediate_nodes`

### other

## Occurrence data list

### app

- [x] `POST /occurrences/search`に空filters / limit 50 / cursor nullを送ると、OccurrenceRdfStoreの検索結果と`dcterms:creator`由来のcreator_user_idを200 OKのJSONで返す`search_occurrences_route_returns_store_results_for_empty_search`
- [x] `POST /occurrences/search`でpage.limitを省略するとdefault limit 50で検索し、OccurrenceRdfStoreにlimit 50が渡る`search_occurrences_route_defaults_limit_to_50_when_omitted`
- [x] `POST /occurrences/search`にscientificName filterを送ると、filterに一致するOccurrenceRdfStoreの検索結果だけを200 OKのJSONで返す`search_occurrences_route_applies_filter_to_store_results`
- [x] `POST /occurrences/search`のliteral exact検索は大文字小文字を区別せず一致する`search_occurrences_route_matches_literal_filter_case_insensitively`
- [x] `POST /occurrences/search`のliteral exact検索は検索値の前後空白を無視して一致する`search_occurrences_route_trims_literal_filter_value`
- [x] `POST /occurrences/search`でfilters[].value_typeがliteralまたはuri以外なら400 Bad Requestを返し、OccurrenceRdfStoreへ検索しない`search_occurrences_route_rejects_invalid_filter_value_type`
- [x] `POST /occurrences/search`でfilters[].matchがexact以外なら400 Bad Requestを返し、OccurrenceRdfStoreへ検索しない`search_occurrences_route_rejects_invalid_filter_match`
- [x] `POST /occurrences/search`でfilters[].predicateが絶対URIでなければ400 Bad Requestを返し、OccurrenceRdfStoreへ検索しない`search_occurrences_route_rejects_non_absolute_filter_predicate`
- [x] 非ログインユーザーが`POST /occurrences/search`で一覧取得したときprivate occurrenceは表示されない`search_occurrences_route_hides_private_occurrences_from_anonymous_user`
- [x] 非ログインユーザーの一覧取得でprivate occurrenceしか取得できない場合、itemsは空でhas_next=false/next_cursor=nullになる`search_occurrences_route_returns_empty_page_when_only_private_results_are_available_to_anonymous_user`
- [x] ログイン済みeditorが`POST /occurrences/search`で一覧取得したとき自分のprivate occurrenceを表示できる`search_occurrences_route_allows_editor_to_view_own_private_occurrence`
- [x] ログイン済みeditorが`POST /occurrences/search`で一覧取得したとき他人のprivate occurrenceは表示されない`search_occurrences_route_hides_other_users_private_occurrences_from_editor`

### service

- [x] `OccurrenceService::search_occurrences` はOccurrenceRdfStoreの検索結果を一覧レスポンスDTOへ変換する`search_occurrences_maps_store_rows_to_response_dto`
- [x] `OccurrenceService::search_occurrences` はfiltersのpredicate/value/value_type/matchをOccurrenceRdfStoreへ渡す`search_occurrences_passes_filters_to_store`
- [x] Fuseki検索filterはIdentification / Event / Location対象述語を各`has*`経由で検索し、unknown predicateはOccurrence直下を検索する`build_search_filter_patterns_routes_predicates_through_intermediate_nodes`
- [x] Fuseki一覧取得はscientificNameをIdentification、recordedByをEventから取得し、子ノードをOccurrenceとして誤認しない`build_search_occurrences_query_reads_intermediate_representative_fields`
- [x] Fuseki置換SPARQLは既存Occurrence本体と接続されたIdentification / Event / Locationを削除してから保存する`build_replace_occurrence_update_includes_intermediate_nodes`

### other

- [x] `FusekiClient::search_occurrences` は実Fusekiに保存されたoccurrenceをfilter付き検索で一覧取得できる（ignored）`fuseki_client_search_occurrences_returns_saved_occurrence_from_real_fuseki`
- [x] `FusekiClient::search_occurrences` はvalue_type=uriのfilterでobject URIに一致するoccurrenceを実Fusekiから取得できる（ignored）`fuseki_client_search_occurrences_matches_uri_filter_object_from_real_fuseki`
- [x] `FusekiClient::search_occurrences` はvalue_type=uriのfilterでrdfs:subClassOf階層を辿り、下位taxonのoccurrenceを実Fusekiから取得できる（ignored）`fuseki_client_search_occurrences_matches_uri_filter_with_subclass_from_real_fuseki`
- [x] `FusekiClient::search_occurrences` はscientificName以外のpredicate filterでも実Fusekiから一致するoccurrenceを取得できる（ignored）`fuseki_client_search_occurrences_matches_non_scientific_name_filter_from_real_fuseki`
- [x] `FusekiClient::search_occurrences` は実Fuseki検索でデータがlimitを超えるとlimit件だけ返しnext_cursorを生成する（ignored）`fuseki_client_search_occurrences_returns_next_cursor_when_results_exceed_limit`
- [x] `FusekiClient::search_occurrences` はcursorを渡すと実Fuseki検索の次ページを取得できる（ignored）`fuseki_client_search_occurrences_uses_cursor_to_return_next_page`

## Real Garage test 統合テスト

- [x] `backend/.env` の S3設定を使って実Garageの `occurrence-media` bucket に一時objectをupload/list/deleteできる（ignored）`garage_client_puts_lists_and_deletes_object_from_real_garage`
- [x] appの`build_app`に本番Garage object storeを入れると、`POST /media`で実Garageに添付データを保存できる（ignored）`upload_media_route_writes_object_to_real_garage`
- [x] appの`build_app`に本番Garage object storeを入れ、`POST /media`で保存した添付データを所有者の`GET /media/{media_id}`で取得すると、同一bytesとMIME typeが返る（ignored）`get_media_route_reads_object_from_real_garage`
- [x] appの`build_app`に本番Garage object storeを入れ、`POST /media`で保存した添付データを所有者の`DELETE /media/{media_id}`で削除すると、実Garage objectが取得不能になりPostgreSQL metadataも削除される（ignored）`delete_media_route_removes_object_from_real_garage_and_metadata_from_postgresql`

- [x] appの`build_app`に実Fusekiと実Garageを同時に入れ、所有者がmedia uploadとpublic occurrence登録を行った後、未ログインの`GET /media/{media_id}`で同一bytesを取得できる（ignored）`get_public_occurrence_media_from_real_fuseki_and_real_garage`
## Real fuseki test 統合テスト

- [x] app経由で`POST /occurrences`に有効sessionと正しいN-Quadsを送ると、実Fusekiに保存されSPARQL ASKで確認できる（ignored）`create_occurrence_route_saves_data_to_real_fuseki`
- [x] `OccurrenceService`と実Fusekiで中間ノード構造を作成・詳細取得・検索・更新置換・削除できる（ignored）`fuseki_occurrence_lifecycle_supports_intermediate_node_structure`
- [x] appの`build_app`に実Fuseki storeを入れると、`POST /occurrences/search`で実Fusekiのoccurrenceを検索できる（ignored）`search_occurrences_route_returns_results_from_real_fuseki`
- [x] appの`build_app`に実Fuseki storeを入れると、`PUT /occurrences/{occurrence_id}`で実Fusekiの既存occurrenceを置換更新できる（ignored）`update_occurrence_route_replaces_existing_occurrence_in_real_fuseki`

- [x] `GET /vocabularies/darwin-core` はDarwin Core graphの述語URIと`localName`をA-Z順で返す`list_darwin_core_terms_route_returns_sorted_terms`
- [x] `FusekiClient` はDarwin Core vocabulary graphから`localName`付き語彙をA-Z順で取得する（ignored）`fuseki_client_lists_darwin_core_terms_from_real_fuseki`

## Auth user summary

### app

- [ ] `GET /users/{user_id}`で既存ユーザーのuser_nameを返す`user_summary_route_returns_user_name_for_existing_user`

## Username update

### service

- [x] 有効sessionと前後空白を含む新しいusernameを渡すと、本人のusernameだけをtrimして更新する`update_user_name_updates_authenticated_user_with_trimmed_value`
- [x] 空または空白だけのusernameを拒否し、既存usernameを変更しない`update_user_name_rejects_blank_value_without_changing_user`

### app

- [x] 有効sessionで`PATCH /auth/me`へusernameを送ると、本人のusernameを更新して新しいユーザー情報を返す`update_current_user_name_route_updates_authenticated_user`
- [x] 未ログインで`PATCH /auth/me`を呼ぶと401を返す`update_current_user_name_route_requires_login`

## Paper Import

### service

- [x] 同一SHA-256のPDFはGarage PUT・GROBID・DB INSERTを行わず既存paperを返す`duplicate_pdf_stops_before_garage_and_grobid`
- [x] 新規PDFはGarage保存、GROBID抽出、全書誌metadataのDB登録を行う`new_pdf_is_stored_extracted_and_inserted_with_metadata`
- [x] Garage PUT失敗時はGROBIDとDB登録を行わない`garage_put_failure_stops_before_grobid_and_insert`
- [x] GROBID失敗時はGarage objectを削除してDB登録しない`grobid_failure_rolls_back_garage_and_does_not_insert`
- [x] DB登録失敗時はGarage objectを削除する`database_failure_after_grobid_rolls_back_garage`
- [x] 実PostgreSQLへの同時同一SHA-256 importは1行だけ保存し、競合側objectを削除する`concurrent_imports_persist_one_global_paper_and_rollback_loser`
- [x] 同時重複でINSERT競合した側は自分のGarage objectを削除して既存paperを返す`concurrent_duplicate_removes_own_object_and_returns_winner`
- [x] 重複PDFは`GROBID_BASE_URL`が不正でもGROBID clientを生成せず`AlreadyImported`を返す`duplicate_pdf_returns_existing_before_invalid_grobid_configuration`
- [x] 初回のDB重複検索が失敗した場合はGarage・GROBIDを呼ばずDB errorを返す`initial_duplicate_lookup_failure_stops_before_external_dependencies`
- [x] 同時重複の競合後に勝者paperが見つからない場合は`ConflictResolutionFailed`を返す`concurrent_duplicate_without_winner_returns_conflict_resolution_failed`
- [x] rollbackのGarage DELETE自体が失敗した場合は`ObjectStoreFailed`を返す`rollback_delete_failure_is_reported_as_object_store_failure`
- [x] `i64`へ安全に保存できないsizeは外部依存を呼ばず拒否する`oversized_service_input_stops_before_external_dependencies`
- [x] 空bucket、0 byte、不正SHA-256を外部依存より前に拒否する`invalid_service_inputs_stop_before_external_dependencies`
- [x] serviceは100 MiBちょうどを受理し、100 MiB + 1 byteを外部依存より前に拒否する`service_enforces_100_mib_pdf_limit`
- [x] GROBIDがDOI・titleを両方取得できなくてもPDFとpaper rowを保存し`MetadataRequired`を返す`new_pdf_without_doi_and_title_is_saved_as_metadata_required`
- [x] DOIまたはtitleの一方でも取得できれば通常の`Imported`を返す`new_pdf_with_minimum_bibliographic_metadata_is_imported`
- [x] 書誌情報未設定の重複PDFは副作用を再実行せず`MetadataRequired`を返す`duplicate_pdf_without_doi_and_title_requires_metadata`

### bibliographic metadata completion

- [ ] ログインユーザーは未設定のDOIだけを正規化して補完できる`authenticated_user_can_complete_missing_doi_with_normalization`
- [ ] ログインユーザーは未設定のtitleだけをtrimして補完できる`authenticated_user_can_complete_missing_title`
- [x] DOI・titleを同時入力しても既存値を上書きせず未設定項目だけ補完する`completion_preserves_existing_grobid_metadata`
- [x] DOI・titleが空または空白だけなら補完を拒否する`completion_rejects_empty_bibliographic_input`
- [ ] 存在しないpaperは`NotFound`になる`completion_returns_not_found_for_missing_paper`
- [ ] 全ユーザー共通で重複排除されたpaperはログイン中の非uploadユーザーも未設定書誌情報を補完できる`authenticated_non_uploader_can_complete_globally_deduplicated_paper`
- [x] 補完後にDOIまたはtitleが存在すれば`requires_bibliographic_input=false`になる`completion_clears_bibliographic_input_requirement`
- [x] 実PostgreSQLでも未設定項目だけを更新し、既存値を原子的に保持する`repository_completes_only_missing_bibliographic_metadata`

### GROBID client / parser

- [x] occurrence抽出bridgeはGarage取得失敗またはExtractor失敗時に`staged`へ戻し、再試行可能にする`service_restores_staged_after_object_store_or_extractor_failure`
- [x] occurrence抽出bridgeは所有者以外または`staged`以外のimportを取得・Extractor呼出なしで拒否する`service_rejects_other_user_or_non_staged_import_without_reading_pdf`
- [x] occurrence抽出bridgeはサイズ・SHA-256が一致していても`%PDF-`で始まらないGarage objectを拒否する`service_rejects_non_pdf_signature_and_returns_import_to_staged`
- [x] occurrence抽出bridgeはExtractor終了後に一時PDFを削除する`service_removes_temporary_pdf_after_extraction`

- [x] llama clientは固定prompt・抽出テキスト・全ページJPEGを同一`/v1/chat/completions` requestのcontent配列へ順序どおり送り、正常なOccurrence JSONを返す`llama_client_sends_multimodal_request_and_parses_occurrences`
- [x] llama clientは空テキスト時に画像参照用メッセージを送り、画像bytesを完全なdata URIとして送る`multimodal_request_uses_image_fallback_for_empty_text_and_encodes_bytes`
- [x] llama clientは500、choicesなし、assistant contentの不正JSON、空scientificName、不正緯度経度を拒否する`llama_client_rejects_upstream_and_invalid_occurrence_responses`
- [x] llama clientはOccurrence JSONの未知フィールドを拒否する`llama_client_rejects_unknown_occurrence_json_fields`
- [x] llama request作成時に空または読めないページ画像を拒否する`multimodal_request_rejects_empty_or_missing_page_image`
- [x] llama clientは接続先URLとモデル名を環境変数から読み込む`llama_client_reads_endpoint_and_model_from_environment`

- [x] fulltext clientは`processFulltextDocument`へPDF、`consolidateHeader=0`、`consolidateCitations=0`、XML Acceptを送信し、TEIを返す`grobid_fulltext_client_sends_expected_request_and_returns_tei`
- [x] fulltext clientは204を`NoContent`、不正または空TEIを`InvalidResponse`として返す`grobid_fulltext_client_handles_no_content_and_invalid_tei`
- [x] TEIの`front`と`body`だけをLLMテキストにし、名前空間付き要素でも`back`の参考文献を除外する`extracts_namespaced_front_and_body_without_bibliography`
- [x] `front`と`body`がないTEIは`back`だけへフォールバックせず、LLMテキストを空にする`does_not_fall_back_to_bibliography_when_front_and_body_are_missing`
- [x] PDF前処理はGROBIDテキストと全JPEGページをページ番号順に保持し、非ページ出力を除外する`preprocess_keeps_tei_text_and_sorts_all_rendered_page_images`
- [x] GROBIDが204でもPDF前処理は空テキストと全ページ画像で成功する`preprocess_continues_with_page_images_when_grobid_has_no_content`
- [x] `pdftoppm`が失敗またはJPEGを出力しない場合、PDF前処理は失敗として返す`preprocess_rejects_renderer_failure_and_empty_output`

- [x] multipart、Accept、consolidateHeaderを正しく送り全書誌metadataを解析する`grobid_client_sends_expected_request_and_parses_all_metadata`
- [x] DOI URL、authors、pagesを正規化し、article numberをpagesから推測しない
- [x] optional field欠落と不正BibTeXを処理する
- [x] GROBIDの204を`NoContent`として返す`grobid_client_maps_no_content_response`
- [x] GROBIDの500をstatus付き`Upstream`として返す`grobid_client_maps_upstream_error_response`
- [x] GROBIDの200＋不正BibTeXを`InvalidResponse`として返す`grobid_client_rejects_invalid_bibtex_response`
- [x] GROBID接続不能を`RequestFailed`として返す`grobid_client_maps_connection_failure`
- [x] GROBID応答timeoutを`RequestFailed`として返す`grobid_client_times_out_slow_response`
- [x] 書誌項目が空の正しいBibTeXは空metadataとして処理する`parses_valid_bibtex_with_no_metadata_fields`
- [x] 不正URL、HTTP以外のURL、0秒timeoutをGROBID設定エラーとして拒否する`grobid_client_rejects_invalid_configuration`
- [x] 閉じ括弧が欠けたBibTeXを不正responseとして拒否する`rejects_truncated_bibtex`

### HTTP

- [x] 有効sessionとPDFで201を返しPostgreSQLへmetadataを保存する`authenticated_pdf_request_returns_created_and_persists_grobid_metadata`
- [x] 未ログイン401、拡張子不正415、MIME不正415、PDF signature不正415、Content-Length超過413を副作用なしで返す
- [x] Content-Lengthなしでも実データが100 MiBを超えた時点で413を返し副作用を残さない`streamed_pdf_over_limit_returns_413_without_side_effects`
- [x] file fieldなしは400でGarage・GROBID・DBに副作用を残さない`missing_file_field_returns_400_without_side_effects`
- [x] filenameがないfile fieldは400でGarage・GROBID・DBに副作用を残さない`missing_filename_returns_400_without_side_effects`
- [x] 壊れたmultipart bodyは400でGarage・GROBID・DBに副作用を残さない`malformed_multipart_returns_400_without_side_effects`
- [x] 未知のmultipart fieldは無視し、後続のfile fieldを処理する`unknown_multipart_field_is_ignored_before_pdf_file`
- [x] 空PDFは400でGarage・GROBID・DBに副作用を残さない`empty_pdf_returns_400_without_side_effects`
- [x] file fieldが複数なら400でGarage・GROBID・DBに副作用を残さない`multiple_file_fields_return_400_without_side_effects`
- [x] `.PDF`拡張子と大文字小文字を含むPDF MIMEを受理する`uppercase_pdf_extension_and_mime_are_accepted`
- [x] HTTP経由の重複PDFは200を返しGarage PUT・GROBIDを再実行しない`duplicate_pdf_request_returns_ok_without_repeating_side_effects`
- [x] GROBID 204・500・不正BibTeXは502を返しGarage objectとDB rowを残さない`grobid_http_failures_return_502_and_rollback`
- [x] Garage PUT失敗はHTTP 502を返しGROBID・DBへ進まない`garage_put_failure_returns_502_without_grobid_or_database_row`
- [x] GROBIDがDOI・titleを取得できないPDFは201と`metadata_required`を返してDBに保存する`paper_import_without_minimum_metadata_returns_metadata_required`
- [x] 書誌情報未設定の重複PDFは副作用を再実行せず200と`metadata_required`を返す`duplicate_pdf_without_metadata_returns_metadata_required_ok`
- [ ] `PATCH /papers/{paper_id}/bibliographic-metadata`はログインユーザーのDOIまたはtitle補完を200で返す`authenticated_user_can_complete_bibliographic_metadata_through_app`
- [ ] 補完APIは未ログイン401、不正UUID・空入力400、存在しないpaper 404を返す`bibliographic_metadata_route_rejects_invalid_requests`
- [ ] ログイン中の非uploadユーザーも全ユーザー共通paperの未設定書誌情報を補完できる`authenticated_non_uploader_can_complete_bibliographic_metadata_through_app`
- [x] 補完APIは既存GROBID値を上書きしない`bibliographic_metadata_route_preserves_existing_values`
- [x] 補完処理のDB失敗を500へmappingする`bibliographic_metadata_database_failure_maps_to_500`

### real services

- [x] 構造的に有効な生成PDFを実GROBIDへ送りtitleを抽出できる（ignored）`real_grobid_extracts_header_from_valid_pdf`
- [x] repositoryに同梱した実在研究論文PDF群から期待するtitle・DOI等を抽出できる（ignored）`real_grobid_extracts_metadata_from_research_paper_fixtures`
- [x] real HTTP・PostgreSQL・Garage・GROBIDで新規PDFを201登録しGarage objectとpapers rowを確認する（ignored）`paper_import_route_works_with_real_postgresql_garage_and_grobid`

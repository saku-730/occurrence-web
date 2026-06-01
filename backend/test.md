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

## Session, Login/Logout

### app

- [x] `POST /auth/login`に JSON body なしでおくると client error`login_route_rejects_missing_json_body`
- [x] `POST /auth/login` に登録済み email と正しい password を送ると 200 OK が返る`login_route_returns_ok_for_registered_user_with_correct_password`
- [x] 存在しない email で `POST /auth/login` しても 401 Unauthorized``
- [x] 間違った、パスワードで`POST /auth/login` しても401
- [x] `POST /auth/login` に正常リクエストでCookiセッション発行される。`login_route_sets_session_cookie_for_registered_user`
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

## Occurrence data

### app

- [x] `POST /occurrences`はCookieがなければ401`create_occurrence_route_requires_login`
- [x] `POST /occurrences`はCookieが無効なら401`create_occurrence_route_returns_unauthorized_for_invalid_session_cookie`
- [x] `POST /occurrences`はCookieが有効なら501`create_occurrence_route_with_valid_session_returns_not_implemented` 未実装だから一旦
- [x] `POST /occurrences`はhttpリクエストのbodyがN-Quads以外は拒否415`create_occurrence_route_rejects_unsupported_content_type`
- [x] `POST /occurrences`はhttpリクエストのbodyが空なら400`create_occurrence_route_rejects_empty_body`
- [x] `POST /occurrences`に有効なユーザーで有効リクエストしたときに201created response`create_occurrence_route_with_valid_session_returns_created`
- [x] `POST /occurrences`に有効 session と正しい N-Quads を送ると、route 経由で保存用 N-Quads が OccurrenceRdfStore に渡される`create_occurrence_route_with_valid_session_saves_nquads_to_store`
- [x] `POST /occurrences`に有効 session と壊れた N-Quads を送ると、400 Bad Request を返し、OccurrenceRdfStore には保存されない。`create_occurrence_route_with_invalid_nquads_returns_bad_request_and_does_not_save`
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
- [x] `GET /occurrences/{occurrence_id}`指定された occurrence_id から occurrence_uri を組み立てる。OccurrenceRdfStore からその occurrence_uri の N-Quads を取得する。存在すれば 200 OK / application/n-quads で返す`get_occurrence_route_returns_nquads_for_existing_occurrence`
- [x] `GET /occurrences/{occurrence_id}`で存在しないoccurrence_idのとき404`get_occurrence_route_returns_not_found_for_missing_occurrence`
- [x] `GET /occurrences/{occurrence_id}`でFusekiへの問い合わせ失敗で502`get_occurrence_route_when_rdf_store_fails_returns_bad_gateway`

### service

- [x] フロントエンドから送られたN-Quadsのblank node subjectをバックエンドが発行したオカレンスuuidに差し替え`replace_all_subjects_with_occurrence_uri_replaces_blank_node_subjects`
- [x] フロントから送られた、N-Quadsにcreate_user_idを付加`add_create_user_id_quad_adds_creator_resource_in_occurrence_graph`
- [x] フロントから送られたN-Quadsをパースしてuser_id追加して、再度シリアライズできるserialize_quads_as_nquads_outputs_named_graph_quads`
- [x] フロントから送られた、N-Quadsを組み立てできる。`build_occurrence_nquads_replaces_subject_and_adds_creator`
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

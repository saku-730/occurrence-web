# テスト一覧

## auth

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
- [ ]  `/auth/pre_register`に正常なemailが送られると、そのemail宛に登録用urlを本文に含むメールが送信される。

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



### repository

- [x] 正常な形式で `pending_registrations` に `email`、`token_hash`、`expires_at` を INSERT できる。保存後、`email` と `token_hash` が一致し、`completed_at` は `NULL`、`expires_at` は現在時刻より未来である。`create_pending_registration_inserts_row`

- [x] 同じ `token_hash` で2回 INSERT しようとすると、1回目は成功し、2回目は `UNIQUE` 制約により失敗する。`create_pending_registration_rejects_duplicate_token_hash`

### mail

- [x] `POST /auth/pre_register` に正常な email を送ると、登録完了URLを含むメール文面が作成される

### other

- [x] `config.rs` の `Config::from_env` が、`APP_HOST`、`APP_PORT`、`APP_BASE_URL`、`DATABASE_URL` を正しく読むことを確認するテストは未作成。

# 14. テスト戦略

## 基本方針

- テスト駆動開発を絶対とする
- 仕様変更はまずテストで表現する
- 正常系だけでなく異常系を必ず書く
- 認可が関係する場合はロール別にテストする
- OpenAPI変更が必要な場合は schema もテストする

---

## テスト分類

### 単体テスト

対象。

- service
- policy
- model
- RDF変換処理
- validation
- URI生成
- password hash
- token hash

### 結合テスト

対象。

- handler / app route
- PostgreSQL repository
- Fuseki client
- Garage S3 client
- mail fake / Mailpit
- CSRF
- session

---

## PostgreSQLテストデータの分離

- 共有または開発用PostgreSQLのtableをテスト開始時に`TRUNCATE`しない
- 認証serviceテストは、public tableと同じ構造を持つconnection単位のtemporary tableを使用する
- temporary tableはテスト用connection終了時に自動削除し、テスト開始前から存在するデータへ影響を与えない
- temporary tableを使用できない結合テストでは、テスト内で生成したUUIDやemailに一致するレコードだけを外部キーの子から順に削除する

---

## 認証テスト

必須。

- login success
- login wrong password
- login nonexistent email
- same message for wrong password and nonexistent email
- session cookie is set
- logout invalidates session
- rolling session extends expiry
- deleted user cannot login
- password reset invalidates sessions

---

## CSRFテスト

必須。

- POST without CSRF returns 403
- PUT without CSRF returns 403
- PATCH without CSRF returns 403
- DELETE without CSRF returns 403
- valid CSRF token allows request
- GET does not require CSRF

---

## 仮登録テスト

必須。

- valid email creates pending registration
- token expires in 1 hour
- resend invalidates old incomplete token
- registered email returns already-registered response
- registered email sends no email
- email send failure rolls back DB changes
- invalid email returns validation error

---

## 本登録テスト

必須。

- valid token creates user
- username is required
- password length is validated
- expired token rejected
- used token rejected
- duplicate use does not create duplicate user
- initial role is editor
- registration does not auto-login

---

## occurrence RDFテスト

必須。

- empty RDF rejected
- Turtle rejected
- N-Quads parse success
- missing graph name rejected
- non-occurrence graph name rejected
- occurrence graph name accepted
- exactly one blank node subject accepted
- multiple blank node subjects rejected
- object blank node rejected
- backend replaces blank node with occurrence URI
- backend preserves occurrence graph
- Identification predicates are moved to `/identifications/1`
- Event predicates are moved to `/events/1`
- Location predicates are moved to `/locations/1`
- unknown predicates remain directly under the occurrence URI
- each generated intermediate node has the specified `rdf:type`
- empty intermediate nodes are not generated
- multiple predicates for the same target node are stored in the same `/1` node
- frontend-sent `hasIdentification`, `hasEvent`, and `hasLocation` are rejected
- frontend-sent `rdf:type` remains directly under the occurrence URI
- frontend-sent `dcterms:creator` rejected
- frontend-sent `dcterms:created` rejected
- frontend-sent `dcterms:modified` rejected
- missing accessRights defaults to public
- valid accessRights accepted
- invalid accessRights rejected
- multiple accessRights rejected
- license URI starting with `https://creativecommons.org/` accepted
- license literal rejected
- multiple license values rejected
- created and modified are xsd:dateTime
- created and modified same on create
- update preserves creator and created
- update changes modified
- detail retrieval returns the normalized intermediate-node structure without flattening
- update rebuilds and replaces intermediate nodes using the same routing rules
- delete removes occurrence quads and generated intermediate-node quads

---

## mediaテスト

必須。

- allowed image extensions accepted
- allowed audio extensions accepted
- allowed video extensions accepted
- disallowed extension rejected
- MIME type is validated
- size limit enforced
- object key does not use original filename
- media_objects row is created
- media URI uses same UUID as media_objects.id
- private bucket access is not exposed directly
- Garage + PostgreSQL success and Jena failure triggers rollback
- orphan media cleanup is not automatic in MVP

---

## searchテスト

必須。

- empty search returns list
- limit default is 50
- limit max is 100
- literal `dwc:scientificName` exact match through `hasIdentification`
- Event predicates are searched through `hasEvent`
- Location predicates are searched through `hasLocation`
- unknown predicates are searched directly under the occurrence URI
- literal search is case-insensitive
- literal search trims whitespace
- URI taxon exact match
- URI taxon search follows `rdfs:subClassOf`
- URI hierarchy search does not apply to literal values
- non-login sees only public
- editor sees public and own private
- admin sees all

---

## audit logテスト

必須。

- pending audit log is created before side effects
- success updates result to success
- failure updates result to failed
- audit log creation failure aborts operation
- login failure is audited
- read/view operation is not audited in MVP

---

## APIテスト

必須。

- all errors use JSON
- validation error has details array
- delete success returns `{ "deleted": true }`
- unauthorized private occurrence returns 404
- OpenAPI contains added endpoints
- OpenAPI contains error response schema

---

## 実行方針

- DBを使うテストは必要に応じて `--test-threads=1`
- 外部サービスはDocker Composeで起動する
- メールは fake mailer または Mailpit を使う
- Fusekiテストはデータセット初期化を行う
- Garageテストはテスト用bucketまたはprefixを使う

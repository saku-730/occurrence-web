# Server setup

## OS インストール

基本はubuntu server LTS最新版

## 必要パッケージインストール

```bash
sudo apt update
sudo apt upgrade 
sudo apt install build-essential
sudo apt install pkg-config libssl-dev
```

### Repository

```bash
sudo snap install gh
```

```bash
gh repo clone saku-730/occurrence-web
```

他設定ファイル等

- /backend/.env
- /postgreSQL/.env
- /fuseki
- /garage

### Docker

```bash
# Add Docker's official GPG key:
sudo apt-get update
sudo apt-get install ca-certificates curl
sudo install -m 0755 -d /etc/apt/keyrings
sudo curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc
sudo chmod a+r /etc/apt/keyrings/docker.asc

# Add the repository to Apt sources:
echo \
  "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu \
  $(. /etc/os-release && echo "${UBUNTU_CODENAME:-$VERSION_CODENAME}") stable" | \
  sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
sudo apt-get update
```

```bash
sudo apt-get install docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
```

```bash
sudo docker run hello-world
```

```bash
sudo usermod -aG docker "$USER"
```

### Garage

download from this link.

https://garagehq.deuxfleurs.fr/download/

#### Truenas mount

```bash
sudo apt install nfs-common cifs-utils
```

```bash
sudo mkdir -p /mnt/truenas/HDD-2TB-1
sudo mount.cifs   //192.168.3.100/HDD-2TB-1   /mnt/truenas/HDD-2TB-1   -o username=saku,vers=3.0,uid=$(id -u),gid=$(id -g),iocharset=utf8
```

↑でマウントがうまくいけば、`/etc/fstab`に以下を書き込み。

```bash
//192.168.3.100/HDD-2TB-1 /mnt/truenas/HDD-2TB-1 cifs credentials=/home/saku/.smbcredentials,vers=3.0,uid=1000,gid=1000,iocharset=utf8,rw,nofail,_netdev,x-systemd.automount 0
```

edit garage.toml 

```toml
metadata_dir =
data_dir = 
```

```bash
export GARAGE_CONFIG_FILE="~/occurrenceweb/garage/garage.toml"
```

```bash
GARAGE_CONFIG_FILE=./garage/garage.toml garage layout assign -z home -c 1T 3e61 -t truenas
GARAGE_CONFIG_FILE=./garage/garage.toml garage layout show
GARAGE_CONFIG_FILE=./garage/garage.toml garage layout apply --version 1
```

```bash
garage bucket create occurrence-media
garage bucket list
```

```bash
garage key create occurrence-web
```

edit Backend `.env` 

```bash
S3_ACCESS_KEY_ID=
S3_SECRET_ACCESS_KEY=
```


### Apache jena fuseki

```bash
sudo apt install unzip
```

download docker file from this link.

https://repo1.maven.org/maven2/org/apache/jena/jena-fuseki-docker/6.1.0/jena-fuseki-docker-6.1.0.zip

#### Darwin Core master data

TDWG の以下5つの Turtle ファイルを元に、Bio-Database 用の N-Quads を作成する。

```text
ac.ttl
dc.ttl
dcterms.ttl
iri.ttl
terms.ttl
```

取得例。

```bash
mkdir -p dwc-source
cd dwc-source

wget https://rs.tdwg.org/dwc/ac.ttl
wget https://rs.tdwg.org/dwc/dc.ttl
wget https://rs.tdwg.org/dwc/dcterms.ttl
wget https://rs.tdwg.org/dwc/iri.ttl
wget https://rs.tdwg.org/dwc/terms.ttl
```

この5ファイルから Darwin Core 語彙本体を作成し、`frontend/content/terms/darwin-core/list.csv` の Bio-Database 固有情報を付加して `darwin_core_master.nq` を生成する。

完成した N-Quads は次の2つの named graph を持つ。

```text
https://bio-database.net/graphs/vocabularies/darwin-core
https://bio-database.net/graphs/app/occurrence-profile
```

役割は次のとおり。

- `https://bio-database.net/graphs/vocabularies/darwin-core`
  - TDWG由来のDarwin Core語彙本体を保持する。
  - Bio-Database固有の設定は入れない。
- `https://bio-database.net/graphs/app/occurrence-profile`
  - Bio-Databaseでその語彙を使用するかを保持する。
  - Bio-Databaseで表示する日本語名を保持する。


Darwin Core公式情報とBio-Database固有情報を別graphにすることで、Darwin Core公式語彙だけを更新する場合とBio-Database側の設定だけを更新する場合を独立して扱える。例えばTDWG側の語彙を更新するときに `vocabularies/darwin-core` graphだけを再作成しても、`occurrence-profile` graphに保存したBio-Database固有設定は残せる。

Bio-Database固有情報として現在追加するのは次の2項目だけとする。

```text
https://bio-database.net/terms/useAtBioDatabase
http://www.w3.org/2004/02/skos/core#prefLabel
```

`useAtBioDatabase` はBio-Database独自の概念なのでBio-Database namespaceを使用する。目的語は `xsd:boolean` で、`true` / `false` を明示的に保存する。

日本語名は既存標準の `skos:prefLabel` を使用し、`@ja` 言語タグを付ける。

例。

```nq
<http://rs.tdwg.org/dwc/terms/scientificName> <https://bio-database.net/terms/useAtBioDatabase> "true"^^<http://www.w3.org/2001/XMLSchema#boolean> <https://bio-database.net/graphs/app/occurrence-profile> .
<http://rs.tdwg.org/dwc/terms/scientificName> <http://www.w3.org/2004/02/skos/core#prefLabel> "学名"@ja <https://bio-database.net/graphs/app/occurrence-profile> .
```

`list.csv`との対応は次のとおり。

| `list.csv` | RDF |
| --- | --- |
| `iri` | 主語IRI |
| `use_at_bio_database` | `bio:useAtBioDatabase` の `xsd:boolean` |
| `label_ja` | `skos:prefLabel` の `@ja` literal |

生成時はDarwin Core語彙graphに実際に存在するIRIを基準とする。

- `list.csv` に同じIRIがあれば `use_at_bio_database` の値を使用する。
- `list.csv` にIRIがなければ `useAtBioDatabase false` を生成する。
- `label_ja` が存在するときだけ日本語 `skos:prefLabel` を生成する。
- `label_ja` が空なら日本語名を推測して生成しない。
- `list.csv` にだけ存在し、Darwin Core語彙本体に存在しないIRIは設定graphにも追加しない。


現在、生成済み N-Quads はあるが、5 TTL と `list.csv` から N-Quads を再生成するスクリプトはリポジトリに未収録。

##### 初回投入

FusekiにまだDarwin Coreデータがない場合は、そのままN-Quadsを投入する。

```bash
FILE='/実際のパス/darwin_core_master_ja.nq'

curl -fsS \
  -u "${FUSEKI_USER}:${FUSEKI_PASSWORD}" \
  -X POST \
  -H 'Content-Type: application/n-quads' \
  --data-binary "@${FILE}" \
  "${FUSEKI_URL}/${FUSEKI_DATASET}/data"
```

##### 既存Darwin Coreデータの置換

既存FusekiのDarwin Core関連データを新しい `darwin_core_master.nq` へ完全に置き換える場合は、対象の2 named graphだけを削除してからN-Quadsを再投入する。

まず既存graphを削除する。

```bash
curl -fsS \
  -u "${FUSEKI_USER}:${FUSEKI_PASSWORD}" \
  -X POST \
  --data-urlencode 'update=
DROP SILENT GRAPH <https://bio-database.net/graphs/vocabularies/darwin-core>;
DROP SILENT GRAPH <https://bio-database.net/graphs/app/occurrence-profile>
' \
  "${FUSEKI_URL}/${FUSEKI_DATASET}/update"
```

その後、新しいN-Quadsを投入する。

```bash
FILE='/実際のパス/darwin_core_master.nq'

curl -fsS \
  -u "${FUSEKI_USER}:${FUSEKI_PASSWORD}" \
  -X POST \
  -H 'Content-Type: application/n-quads' \
  --data-binary "@${FILE}" \
  "${FUSEKI_URL}/${FUSEKI_DATASET}/data"
```

この操作で削除するのはDarwin Core関連の2 named graphだけであり、Occurrence RDF、GBIF Backboneなど他のnamed graphは削除しない。

##### 投入確認

投入後はgraphごとのtriple数を確認する。

```bash
curl -fsS \
  -u "${FUSEKI_USER}:${FUSEKI_PASSWORD}" \
  --get \
  --data-urlencode 'query=
SELECT ?g (COUNT(*) AS ?count)
WHERE {
  GRAPH ?g { ?s ?p ?o }
  FILTER (?g IN (
    <https://bio-database.net/graphs/vocabularies/darwin-core>,
    <https://bio-database.net/graphs/app/occurrence-profile>
  ))
}
GROUP BY ?g
ORDER BY ?g
' \
  -H 'Accept: application/sparql-results+json' \
  "${FUSEKI_URL}/${FUSEKI_DATASET}/query"
```

現在の生成データであれば期待値は次のとおり。

```text
https://bio-database.net/graphs/vocabularies/darwin-core   3654
https://bio-database.net/graphs/app/occurrence-profile      799
```

Bio-Database固有設定の内容を確認する場合は次のSPARQLを使用できる。

```sparql
SELECT ?term ?enabled ?labelJa
WHERE {
  GRAPH <https://bio-database.net/graphs/app/occurrence-profile> {
    ?term <https://bio-database.net/terms/useAtBioDatabase> ?enabled .
    OPTIONAL {
      ?term <http://www.w3.org/2004/02/skos/core#prefLabel> ?labelJa .
      FILTER(LANG(?labelJa) = "ja")
    }
  }
}
LIMIT 20
```

詳細なデータモデルとbackend側の利用方針は `spec/17_darwin_core_candidates.md` を参照する。

#### GBIF Backbone master data

GBIF Backbone の `simple.txt.gz` を取得し、リポジトリ内の変換ツールで N-Quads に変換する。

gzip は展開不要。

```bash
wget https://hosted-datasets.gbif.org/datasets/backbone/current/simple.txt.gz
```

変換。

```bash
cd ~/occurrence-web/tools/gbif-backbone-to-rdf
cargo build --release

cargo run --release -- \
  /path/to/simple.txt.gz \
  gbif-backbone.nq
```

生成される分類マスタの named graph。

```text
https://bio-database.net/graphs/taxonomy/gbif-backbone
```

GBIF Backbone は大きすぎるので Web API 経由では投入せず `tdb2.tdbloader` を使用する。

```bash
docker compose stop fuseki
```

```bash
FILE='/home/saku/gbif-backbone.nq'
FILE_ABS="$(realpath "${FILE}")"

docker compose run --rm --no-deps \
  -v "${FILE_ABS}:/workspace/gbif-backbone.nq:ro" \
  --entrypoint tdb2.tdbloader \
  fuseki \
  --loader=phased \
  --loc /fuseki/databases/occurrence \
  /workspace/gbif-backbone.nq
```

### PostgreSQL

install go

```bash
wget https://go.dev/dl/go1.26.5.linux-amd64.tar.gz
rm -rf /usr/local/go && sudo tar -C /usr/local -xzf go1.26.5.linux-amd64.tar.gz
```

```bash
export PATH=$PATH:/usr/local/go/bin
go version
```

```bash
echo 'export PATH=$PATH:/usr/local/go/bin' >> ~/.bashrc
source ~/.bashrc
```

install goose

```bash
go install github.com/pressly/goose/v3/cmd/goose@latest
```

```bash
export PATH="$(go env GOPATH)/bin:$PATH"
hash -r
which goose
goose -version

echo 'export PATH="$(go env GOPATH)/bin:$PATH"' >> ~/.bashrc
echo 'export PATH="$(go env GOPATH)/bin:$PATH"' >> ~/.profile

source ~/.bashrc
which goose
goose -version
```

```bash
cd ~/occurrence-web/postgreSQL
source .env
goose status
```

```bash
goose up
```

### RUST

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

```bash
source "$HOME/.cargo/env"
rustc --version  
cargo --version
```

```bash
cargo build --release
```

```bash
cargo run
```

### Next.js

download page

https://nodejs.org/en/download/current

```bash
cd ~/occurrence-web/frontend
npm install
npm run build
```

```bash
npm run start
```



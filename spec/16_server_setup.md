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

この5ファイルから Bio-Database 用の metadata を付与して `darwin_core_master.nq` を生成する。
主に次の named graph を使用する。

```text
https://bio-database.net/graphs/vocabularies/darwin-core
https://bio-database.net/graphs/app/occurrence-profile
```

現在、生成済み N-Quads はあるが、5 TTL から N-Quads を再生成するスクリプトはリポジトリに未収録。

Fuseki への投入。

```bash
FILE='/実際のパス/darwin_core_master.nq'

curl -fsS \
  -u "${FUSEKI_USER}:${FUSEKI_PASSWORD}" \
  -X POST \
  -H 'Content-Type: application/n-quads' \
  --data-binary "@${FILE}" \
  "${FUSEKI_URL}/${FUSEKI_DATASET}/data"
```

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



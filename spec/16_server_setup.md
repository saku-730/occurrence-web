# Server setup

## OS インストール

基本はubuntu server LTS最新版

## 必要パッケージインストール

```bash
sudo apt update

sudo apt upgrade 
```

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

### Garage

download from this link.

https://garagehq.deuxfleurs.fr/download/


### Apache jena fuseki

```bash
sudo apt install unzip
```

download docker file from this link.

https://repo1.maven.org/maven2/org/apache/jena/jena-fuseki-docker/6.1.0/jena-fuseki-docker-6.1.0.zip


Darwin core 導入。

```bash
FILE='/実際のパス/darwin_core_master_single_graph.nq'

curl -fsS \
  -u "${FUSEKI_USER}:${FUSEKI_PASSWORD}" \
  -X POST \
  -H 'Content-Type: application/n-quads' \
  --data-binary "@${FILE}" \
  "${FUSEKI_URL}/${FUSEKI_DATASET}/data"
```

### RUST

### Next.js

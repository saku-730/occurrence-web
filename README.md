# Bio-Database / occurrence-web

Bio-Database is a web application for managing biodiversity occurrence records and related media and paper-derived data. The backend is written in Rust with Axum, the frontend uses Next.js, structured application data is stored in PostgreSQL, occurrence data is stored as RDF in Apache Jena Fuseki, and binary objects such as media and imported PDFs are stored in Garage through its S3-compatible API.

This README focuses on getting a development environment running. More detailed infrastructure and data-loading notes are kept under [`spec/`](spec/), especially [`spec/16_server_setup.md`](spec/16_server_setup.md).

## Architecture

The main runtime components are:

- **Frontend:** Next.js
- **Backend:** Rust / Axum
- **Application database:** PostgreSQL
- **RDF store:** Apache Jena Fuseki
- **Object storage:** Garage (S3-compatible API)
- **Paper metadata extraction:** GROBID
- **Paper occurrence extraction:** an OpenAI-compatible llama.cpp endpoint
- **Japanese address parsing:** Digital Agency ABR PostgreSQL
- **Coordinate geocoding:** Nominatim
- **Development mail server:** Mailpit

ABR and Nominatim have deliberately separate roles. ABR is used only to split Japanese addresses into administrative components. Coordinates are obtained only from Nominatim.

## 1. Prerequisites

The development environment is primarily intended for Linux/Ubuntu.

Install the basic build/runtime packages:

```bash
sudo apt update
sudo apt install -y \
  build-essential \
  pkg-config \
  libssl-dev \
  poppler-utils \
  postgresql-client \
  ca-certificates \
  curl \
  git
```

You also need:

- Docker Engine and the Docker Compose plugin
- Rust and Cargo
- Node.js and npm
- Go and [`goose`](https://github.com/pressly/goose) for PostgreSQL migrations
- Garage for S3-compatible object storage

Install Rust with rustup:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Install `goose` after installing Go:

```bash
go install github.com/pressly/goose/v3/cmd/goose@latest
export PATH="$(go env GOPATH)/bin:$PATH"
```

## 2. Clone the repository

```bash
git clone https://github.com/saku-730/occurrence-web.git
cd occurrence-web
git switch main
git pull --ff-only
```

## 3. Start the bundled development services

The root [`compose.yaml`](compose.yaml) defines:

- PostgreSQL on `127.0.0.1:5432`
- Mailpit SMTP on `127.0.0.1:1025`
- Mailpit Web UI on `127.0.0.1:8025`
- GROBID on `127.0.0.1:8070`
- Fuseki on `127.0.0.1:3033`

PostgreSQL, Mailpit, and GROBID can be started immediately:

```bash
docker compose up -d postgres mailpit grobid
```

Check them with:

```bash
docker compose ps
```

### Fuseki configuration

The local `fuseki/` directory is intentionally ignored by Git because it contains deployment-specific configuration, credentials, and database files. Before starting Fuseki, provide at least the files expected by `compose.yaml`:

```text
fuseki/
├── config.ttl
├── databases/
└── secrets/
    └── passwd
```

Then start Fuseki:

```bash
docker compose up -d fuseki
```

The backend normally uses a dataset URL such as:

```text
http://127.0.0.1:3033/occurrence
```

See [`spec/16_server_setup.md`](spec/16_server_setup.md) for the full Fuseki setup and the Darwin Core / GBIF Backbone data-loading procedure.

## 4. Initialize the application PostgreSQL database

The bundled development PostgreSQL container uses:

```text
host:     127.0.0.1
port:     5432
database: occurrence_web
user:     admin
password: occurrence_password
```

Apply the migrations in `postgreSQL/migrations`:

```bash
export DATABASE_URL='postgres://admin:occurrence_password@127.0.0.1:5432/occurrence_web?sslmode=disable'

goose -dir postgreSQL/migrations postgres "$DATABASE_URL" up
```

Check migration status with:

```bash
goose -dir postgreSQL/migrations postgres "$DATABASE_URL" status
```

## 5. Set up Garage

Garage is not started by Docker Compose. The backend talks to Garage through its S3-compatible HTTP API.

The local `garage/` directory is intentionally ignored by Git. Create a development configuration at:

```text
garage/garage.toml
```

Then start Garage from the repository root:

```bash
GARAGE_CONFIG_FILE=./garage/garage.toml garage server
```

Create the bucket and application key after initializing the Garage layout:

```bash
GARAGE_CONFIG_FILE=./garage/garage.toml garage bucket create occurrence-media
GARAGE_CONFIG_FILE=./garage/garage.toml garage key create occurrence-web
```

Record the generated access key and secret key; they are required in `backend/.env`.

For the complete Garage installation, layout, persistent-storage, and TrueNAS examples, see [`spec/16_server_setup.md`](spec/16_server_setup.md).

## 6. Set up the Digital Agency ABR database

Bio-Database uses the official [`digital-go-jp/abr-geocoder`](https://github.com/digital-go-jp/abr-geocoder) project to obtain the Address Base Registry data.

**Current `main` reads the ABR PostgreSQL database directly. It does not call the `abrg_app` HTTP API.** Therefore, Bio-Database needs the imported ABR PostgreSQL database to be running, but it does not need the ABR DuckDB cache or the `abrg_app` API server.

Clone the official ABR repository separately:

```bash
cd ~
git clone https://github.com/digital-go-jp/abr-geocoder.git
cd abr-geocoder
cp .env.example .env
```

Edit `.env` and set a strong `DB_PASSWORD`. If the application PostgreSQL already uses host port `5432`, use another host port for ABR, for example:

```env
DB_PORT=5433
DB_USER=postgres
DB_PASSWORD=change-this-password
DB_NAME=abrdb
DB_SSLMODE=disable
```

Start the ABR PostgreSQL server:

```bash
docker compose up -d postgres
```

Initialize nationwide address data. Bio-Database does not use ABR coordinates, so `--pos` is intentionally omitted:

```bash
docker compose run --rm abrdb_app init --pref all --category all
```

Download and import the ABR data:

```bash
docker compose run --rm abrdb_app import
```

The backend can then connect directly to this database, for example:

```env
ABR_DATABASE_URL=postgres://postgres:change-this-password@127.0.0.1:5433/abrdb
```

For the current Bio-Database implementation, these ABR API-server commands are **not required**:

```bash
# Not required by occurrence-web current main:
docker compose run --rm abrg_app cache build
docker compose up -d abrg_app
```

Nominatim is used separately for final latitude/longitude lookup. The backend currently uses the public `https://nominatim.openstreetmap.org/` endpoint, serializes requests, enforces a minimum one-second interval, and caches repeated queries in memory.

## 7. Configure the backend

Copy the example environment file:

```bash
cp backend/.env.example backend/.env
```

For a local development environment, the important values are approximately:

```env
# Application
APP_HOST=127.0.0.1
APP_PORT=3001
APP_BASE_URL=http://127.0.0.1:3001
APP_ENV=development
COOKIE_SECURE=false

# Application PostgreSQL
DATABASE_URL=postgres://admin:occurrence_password@127.0.0.1:5432/occurrence_web

# Mailpit
SMTP_HOST=127.0.0.1
SMTP_PORT=1025
SMTP_USERNAME=
SMTP_PASSWORD=
SMTP_TLS=none
MAIL_FROM=no-reply@example.com

# Fuseki
FUSEKI_BASE_URL=http://127.0.0.1:3033/occurrence
FUSEKI_USER=<your-fuseki-user>
FUSEKI_PASSWORD=<your-fuseki-password>

# Garage / S3-compatible API
S3_ENDPOINT=http://127.0.0.1:<garage-s3-port>
S3_REGION=garage
S3_BUCKET=occurrence-media
S3_ACCESS_KEY=<garage-access-key>
S3_SECRET_KEY=<garage-secret-key>
S3_FORCE_PATH_STYLE=true

# GROBID
GROBID_BASE_URL=http://127.0.0.1:8070

# Digital Agency ABR PostgreSQL
ABR_DATABASE_URL=postgres://postgres:<abr-password>@127.0.0.1:5433/abrdb
```

`DATABASE_URL`, `FUSEKI_BASE_URL`, `FUSEKI_USER`, `FUSEKI_PASSWORD`, and the Garage/S3 settings must be valid for the backend to start successfully.

`ABR_DATABASE_URL` is optional at process startup. If it is absent or unusable, registration can fall back to free-form Nominatim geocoding instead of ABR-assisted address splitting.

### Paper extraction with llama.cpp

Paper occurrence extraction expects an OpenAI-compatible chat-completions endpoint. Configure it when using the paper extraction feature:

```env
LLAMA_CHAT_COMPLETIONS_URL=http://127.0.0.1:<port>/v1/chat/completions
LLAMA_MODEL=<model-name>
```

GROBID defaults to `http://127.0.0.1:8070` when `GROBID_BASE_URL` is not set.

## 8. Run the backend

```bash
cd backend
cargo run
```

With the development settings above, the backend listens on:

```text
http://127.0.0.1:3001
```

For an optimized binary:

```bash
cargo build --release
./target/release/backend
```

## 9. Configure and run the frontend

From the repository root:

```bash
cp frontend/.env.example frontend/.env.local
```

The default development configuration points Next.js server-side rewrites to the backend:

```env
BACKEND_URL=http://127.0.0.1:3001
```

`NEXT_PUBLIC_MAP_STYLE_URL` is optional. When omitted, the map uses the application's default OpenFreeMap style.

Install dependencies and start Next.js:

```bash
cd frontend
npm ci
npm run dev
```

The frontend listens on:

```text
http://127.0.0.1:3002
```

Useful frontend commands:

```bash
npm run lint
npm run typecheck
npm run build
npm run start
```

## 10. Development URLs

With the setup above:

| Component | URL / endpoint |
| --- | --- |
| Frontend | `http://127.0.0.1:3002` |
| Backend | `http://127.0.0.1:3001` |
| PostgreSQL | `127.0.0.1:5432` |
| Fuseki | `http://127.0.0.1:3033` |
| GROBID | `http://127.0.0.1:8070` |
| Mailpit Web UI | `http://127.0.0.1:8025` |
| Mailpit SMTP | `127.0.0.1:1025` |
| ABR PostgreSQL | typically `127.0.0.1:5433` |

## 11. Master data

A fully useful Bio-Database deployment also needs RDF master data in Fuseki.

The repository currently documents two important datasets:

- Darwin Core vocabulary and the Bio-Database occurrence profile
- GBIF Backbone taxonomy

The detailed generation/loading commands are intentionally kept in [`spec/16_server_setup.md`](spec/16_server_setup.md), because GBIF Backbone loading uses `tdb2.tdbloader` and is substantially heavier than the normal application startup procedure.

## 12. Production build

Build the backend and frontend separately:

```bash
cd backend
cargo build --release

cd ../frontend
npm ci
npm run build
```

The repository includes [`start-production.sh`](start-production.sh). It expects:

- the release backend binary to already exist
- the Next.js production build to already exist
- PostgreSQL and Fuseki to already be running and reachable
- a valid Garage configuration
- the backend and frontend environment files to already be configured

Run it from the repository root:

```bash
./start-production.sh
```

The script starts Garage, the Rust backend, and the Next.js production server as one foreground process group. Production deployments should normally replace this simple launcher with an appropriate service manager such as systemd or another supervised deployment mechanism.

When `APP_ENV=production`, `COOKIE_SECURE=true` is required by the backend.

## 13. Testing

Backend:

```bash
cd backend
cargo test
```

Frontend:

```bash
cd frontend
npm run lint
npm run typecheck
npm run build
```

Some integration tests require external services and are intentionally ignored unless the corresponding real service is available.

## Repository layout

```text
occurrence-web/
├── backend/                 Rust/Axum backend
├── frontend/                Next.js frontend
├── postgreSQL/migrations/   Application PostgreSQL migrations
├── jena-fuseki-docker-6.1.0/ Fuseki Docker build context
├── for-fuseki/              RDF data/tools used for Fuseki
├── tools/                   Data conversion/import utilities
├── spec/                    Architecture and detailed setup specifications
├── compose.yaml             Development infrastructure services
├── start-production.sh      Simple production launcher
└── LICENSE
```

Local `fuseki/`, `garage/`, and `.env` files are intentionally not committed.

## License

This project is licensed under the **GNU Affero General Public License v3.0 only (`AGPL-3.0-only`)**. See [`LICENSE`](LICENSE).

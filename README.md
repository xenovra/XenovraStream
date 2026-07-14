![xenovradrive-github-logo](https://github.com/Xenovra/XenovraDrive/assets/55978340/db39e76f-4119-41c1-bbfd-9b59f40ab626)

[<img alt="GitHub Workflow Status (with event)" src="https://img.shields.io/github/actions/workflow/status/xenovra/XenovraDrive/docker-image.yml?style=plastic&logo=github">](https://github.com/xenovra/XenovraDrive/actions)
[<img alt="Latest release" src="https://img.shields.io/github/v/release/xenovra/XenovraDrive?style=plastic&logo=github&color=success">](https://github.com/xenovra/XenovraDrive/releases)
[<img alt="Dockerhub latest" src="https://img.shields.io/badge/dockerhub-latest-blue?logo=docker&style=plastic">](https://hub.docker.com/r/xenovra/xenovradrive)
[<img alt="GitHub Packages" src="https://img.shields.io/badge/ghcr.io-latest-24292e?logo=github&style=plastic">](https://github.com/xenovra/XenovraDrive/pkgs/container/xenovradrive)
[<img alt="Docker Image Size (tag)" src="https://img.shields.io/docker/image-size/xenovra/xenovradrive/latest?style=plastic&logo=docker&color=gold">](https://hub.docker.com/r/xenovra/xenovradrive/tags?page=1&name=latest)
[<img alt="Any platform" src="https://img.shields.io/badge/platform-any-green?style=plastic&logo=linux&logoColor=white">](https://github.com/xenovra/XenovraDrive)

_Lightweight, self-hosted cloud storage that uses **Telegram** as its storage backend — so it doesn't consume your server filesystem or any paid cloud storage underneath the hood._

XenovraDrive is aimed to take as small disk space as possible. It does not need any code interpreter/platform to run — the whole app is a **`FROM scratch` binary just a few megabytes in size**. It uses Postgres as a database and tries hard to economize space by not creating unneeded fields and tables and by picking proper datatypes.

The platform can be used as a personal drive (on your own server or a local machine) or as a multi-user platform with multiple storages. Since it also exposes a REST API, you can use it as a storage layer in your backend, similar to [NextCloud](https://nextcloud.com/), [AWS S3](https://aws.amazon.com/s3/) or S3-compatible services like [MinIO](https://min.io/).

## ✨ Features

- **Telegram-backed storage** — files are split into chunks and stored in a Telegram channel through a bot, so you pay nothing for storage.
- **Unlimited file size** — files are chunked on upload and reassembled on download, working around Telegram's per-file limit.
- **Clean, modern web UI** (SolidJS) — responsive interface with folders, file/folder info, and a polished light theme.
- **Live upload progress bar** — real-time browser→server progress, then a "processing" state while the server forwards the file to Telegram.
- **Delete syncs to Telegram** — removing a file (or a whole folder) also deletes its underlying messages from the Telegram channel, so nothing is left behind.
- **Per-user access control** — grant, change or revoke access to a storage per user (Viewer / Can edit / Admin).
- **Multiple storage workers** — add more Telegram bots to a storage to work around per-bot rate limits and upload/download faster.
- **JWT authentication** with automatic superuser bootstrap on first run.
- **Tiny & portable** — a multi-megabyte static image that runs anywhere Docker does; published to both Docker Hub and GitHub Packages.

## 🚀 Installation

This project is aimed at running the app in a container, so the primary way to run it is via [Docker](https://www.docker.com/). You can also build it from source.

> **NOTE:** XenovraDrive uses [Postgres](https://www.postgresql.org/) as a database. If you run it from source or run only the XenovraDrive image, you need a Postgres instance running and reachable on your network.

### Pull the image

```sh
# GitHub Packages (ghcr.io)
docker pull ghcr.io/xenovra/xenovradrive:latest

# or Docker Hub
docker pull xenovra/xenovradrive:latest
```

<details>
  <summary><b>Docker Compose with pre-built image</b> <i>(recommended)</i></summary>

The simplest way to run and manage the app.

1. Create a directory for the app and enter it:

```sh
mkdir xenovradrive && cd xenovradrive
```

2. Add a `docker-compose.yml`:

```yaml
volumes:
  xenovradrive-db-volume:
    name: xenovradrive-db-volume

services:
  xenovradrive:
    container_name: xenovradrive
    image: ghcr.io/xenovra/xenovradrive:latest   # or xenovra/xenovradrive:latest
    env_file:
      - .env
    ports:
      - ${PORT}:8000
    restart: unless-stopped
    depends_on:
      - db

  db:
    container_name: xenovradrive_db
    image: postgres:15.0-alpine
    environment:
      POSTGRES_USER: ${DATABASE_USER}
      POSTGRES_PASSWORD: ${DATABASE_PASSWORD}
    restart: unless-stopped
    volumes:
      - xenovradrive-db-volume:/var/lib/postgresql/data
```

3. Add a `.env` file. **Set your own superuser email, password and secret key**:

```env
PORT=8000
WORKERS=4
CHANNEL_CAPACITY=32
SUPERUSER_EMAIL=<YOUR-EMAIL>
SUPERUSER_PASS=<YOUR-PASSWORD>
ACCESS_TOKEN_EXPIRE_IN_SECS=1800
REFRESH_TOKEN_EXPIRE_IN_DAYS=14
SECRET_KEY=<YOUR-SECRET-KEY>
TELEGRAM_API_BASE_URL=https://api.telegram.org

DATABASE_USER=xenovradrive
DATABASE_PASSWORD=xenovradrive
DATABASE_NAME=xenovradrive
DATABASE_HOST=db
DATABASE_PORT=5432
```

Generate a strong secret key with:

```sh
openssl rand -hex 32
```

4. Run it:

```sh
docker compose up -d
```

Open http://localhost:8000 (or `http://<YOUR-PUBLIC-IP>:8000` on a server) and sign in with your superuser credentials. Check logs with `docker logs -f xenovradrive`.

</details>

<details>
  <summary><b>Docker Compose from source</b></summary>

Aimed at the development process.

```sh
git clone git@github.com:xenovra/XenovraDrive.git
cd XenovraDrive
cp ./.env.example ./.env   # then edit it
make up
```

Open http://localhost:8000 and check logs with `docker logs -f xenovradrive`.

</details>

<details>
  <summary><b>From source</b></summary>

The most involved way. Requires [Cargo](https://github.com/rust-lang/cargo), [Node.js](https://nodejs.org/en), [pnpm](https://pnpm.io/) and [Postgres](https://www.postgresql.org/).

```sh
git clone git@github.com:xenovra/XenovraDrive.git
cd XenovraDrive

# build the server
cd xenovradrive && cargo build --release && cd ..

# build the UI
cd ui && pnpm install && VITE_API_BASE=/api pnpm run build && cd ..
```

Serve the built UI (`ui/dist`) next to the binary, make sure Postgres is reachable, set the environment variables from [.env.example](https://github.com/xenovra/XenovraDrive/blob/main/.env.example), then run the `xenovradrive` binary.

</details>

<br/>

It's recommended to put a HTTP reverse-proxy like [Nginx](https://www.nginx.com/) or [Traefik](https://traefik.io/traefik/) in front of the app for TLS.

## 📖 Usage

The platform is built around the **"storage"** concept. Every storage is a separate file system, like different volumes on a drive — you can create files and folders, download files, view file/folder info and delete them, much like Google Drive. Each storage is backed by its own Telegram channel where the data actually lives.

Storages use **"storage workers"** — Telegram bots that upload and download files through the Telegram API. Add the bot to your channel as an **administrator with permission to post and delete messages**.

### Telegram API limitations & how XenovraDrive works around them

- **Rate limits (RPM):** add extra storage workers (bots) to a storage to spread the load and upload/download faster. One user can create up to 20 bots.
- **File size:** Telegram caps single files, so XenovraDrive splits uploads into chunks, stores them separately, and reassembles them on download — allowing effectively unlimited file sizes.

### In-storage features

- [x] Upload file (with live progress)
- [x] Download file
- [x] Create folder
- [x] Get file / folder info
- [x] Delete file / folder (also removes the data from Telegram)

### Access control

Manage access to your storages by granting access to other users. Three roles are available:

- **Viewer**
- **Can edit**
- **Admin**

You can grant, change or revoke (delete) access for other users at any time.

## 🗺️ Future plans

Planned and considered improvements (contributions welcome):

- [ ] Move / rename files and folders
- [ ] Multi-file and drag-and-drop uploads
- [ ] Resumable and parallel chunk transfers for higher throughput
- [ ] Trash / recycle bin before permanent deletion
- [ ] Search within a storage
- [ ] File previews and thumbnails (images, video, PDF)
- [ ] Public / password-protected share links
- [ ] Dark mode for the web UI
- [ ] Storage usage stats and quotas per user
- [ ] Documented, stable public REST API + client SDKs
- [ ] Optional S3-compatible API layer

Have an idea or a feature you'd like to see? Open an issue — feedback drives the roadmap.

## 🤝 Contributing

Highly welcome! Open issues or pick existing ones and send PRs.

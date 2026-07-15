use std::time::Duration;

use sqlx::PgPool;

use crate::{
    common::{db::pool::get_pool, password_manager::PasswordManager},
    config::Config,
    errors::XenovraStreamError,
    models::users::InDBUser,
    repositories::users::UsersRepository,
};

#[inline]
pub async fn create_db(dsn: &str, dbname: &str, max_connection: u32, timeout: Duration) {
    let db = get_pool(dsn, max_connection, timeout).await;

    tracing::debug!("creating database");

    let result = sqlx::query(format!("CREATE DATABASE {dbname}").as_str())
        .execute(&db)
        .await;

    match &result {
        Ok(_) => {
            tracing::debug!("created database");
            return;
        }
        Err(sqlx::Error::Database(dbe)) => {
            if let Some(code) = dbe.code() {
                if code == "42P04" {
                    tracing::debug!("database already exists; skipping");
                    return;
                }
            }
        }
        _ => (),
    };

    result.unwrap();
}

#[inline]
pub async fn init_db(db: &PgPool) {
    tracing::debug!("initing database");

    let mut transaction = db.begin().await.unwrap();

    for statement in [
        "
        CREATE TABLE IF NOT EXISTS users (
            id            UUID         PRIMARY KEY,
            email         VARCHAR(255) NOT NULL UNIQUE,
            password_hash VARCHAR(255) NOT NULL
        );
    ",
        "
        CREATE TABLE IF NOT EXISTS storages (
            id      UUID         PRIMARY KEY,
            name    VARCHAR(255) NOT NULL,
            chat_id BigInt       NOT NULL UNIQUE
        );

    ",
        "
        CREATE TABLE IF NOT EXISTS storage_workers (
            id         UUID         PRIMARY KEY,
            name       VARCHAR(255) NOT NULL,
            token      VARCHAR(255) NOT NULL UNIQUE,
            user_id    UUID         NOT NULL REFERENCES users
                                            ON DELETE CASCADE 
                                            ON UPDATE CASCADE,
            storage_id UUID         REFERENCES storages
        );

    ",
        "
        DO
        $$
        BEGIN
        IF NOT EXISTS (
            SELECT *
            FROM pg_type typ
            INNER JOIN pg_namespace nsp ON nsp.oid = typ.typnamespace
            WHERE nsp.nspname = current_schema() AND typ.typname = 'access_type'
        ) THEN
            CREATE TYPE access_type AS ENUM ('r', 'w', 'a');
        END IF;
        END;
        $$;
    ",
        "
        CREATE TABLE IF NOT EXISTS access (
            id          UUID        PRIMARY KEY,
            user_id     UUID        NOT NULL REFERENCES users
                                            ON DELETE CASCADE 
                                            ON UPDATE CASCADE,
            storage_id  UUID        NOT NULL REFERENCES storages
                                            ON DELETE CASCADE 
                                            ON UPDATE CASCADE,
            access_type access_type NOT NULL,

            UNIQUE(user_id, storage_id)
        );
    ",
        "
        DO
        $$
        BEGIN
        IF NOT EXISTS (
            SELECT *
            FROM pg_type typ
            INNER JOIN pg_namespace nsp ON nsp.oid = typ.typnamespace
            WHERE nsp.nspname = current_schema() AND typ.typname = 'video_status'
        ) THEN
            CREATE TYPE video_status AS ENUM (
                'queued', 'transcoding', 'uploading', 'ready', 'failed'
            );
        END IF;
        END;
        $$;
    ",
        "
        CREATE TABLE IF NOT EXISTS videos (
            id                UUID         PRIMARY KEY,
            public_id         VARCHAR(32)  NOT NULL UNIQUE,
            title             VARCHAR(255) NOT NULL,
            original_filename VARCHAR(255) NOT NULL,
            size              BigInt       NOT NULL,
            duration_secs     Double Precision,
            storage_id        UUID         NOT NULL REFERENCES storages
                                                    ON DELETE CASCADE
                                                    ON UPDATE CASCADE,
            user_id           UUID         NOT NULL REFERENCES users
                                                    ON DELETE CASCADE
                                                    ON UPDATE CASCADE,
            status            video_status NOT NULL,
            progress          SmallInt     NOT NULL DEFAULT 0,
            error             TEXT,
            created_at        TIMESTAMP    NOT NULL DEFAULT NOW(),
            updated_at        TIMESTAMP    NOT NULL DEFAULT NOW()
        );
    ",
        "
        CREATE INDEX IF NOT EXISTS videos_user_id_idx ON videos (user_id);
    ",
        "
        CREATE TABLE IF NOT EXISTS renditions (
            id              UUID        PRIMARY KEY,
            video_id        UUID        NOT NULL REFERENCES videos
                                                ON DELETE CASCADE
                                                ON UPDATE CASCADE,
            name            VARCHAR(16) NOT NULL,
            width           Int         NOT NULL,
            height          Int         NOT NULL,
            bandwidth       Int         NOT NULL,
            codecs          VARCHAR(64) NOT NULL,
            target_duration Int         NOT NULL,

            UNIQUE (video_id, name)
        );
    ",
        "
        CREATE TABLE IF NOT EXISTS segments (
            id                  UUID             PRIMARY KEY,
            rendition_id        UUID             NOT NULL REFERENCES renditions
                                                        ON DELETE CASCADE
                                                        ON UPDATE CASCADE,
            position            Int              NOT NULL,
            duration            Double Precision NOT NULL,
            size                Int              NOT NULL,
            telegram_file_id    VARCHAR(255)     NOT NULL,
            telegram_message_id BigInt           NOT NULL DEFAULT 0,

            UNIQUE (rendition_id, position)
        );
    ",
        "
        DO
        $$
        BEGIN
        IF NOT EXISTS (
            SELECT *
            FROM pg_type typ
            INNER JOIN pg_namespace nsp ON nsp.oid = typ.typnamespace
            WHERE nsp.nspname = current_schema() AND typ.typname = 'job_state'
        ) THEN
            CREATE TYPE job_state AS ENUM ('pending', 'running', 'done', 'failed');
        END IF;
        END;
        $$;
    ",
        "
        CREATE TABLE IF NOT EXISTS transcode_jobs (
            id          UUID      PRIMARY KEY,
            video_id    UUID      NOT NULL UNIQUE REFERENCES videos
                                                ON DELETE CASCADE
                                                ON UPDATE CASCADE,
            source_path TEXT      NOT NULL,
            state       job_state NOT NULL,
            attempts    SmallInt  NOT NULL DEFAULT 0,
            last_error  TEXT,
            created_at  TIMESTAMP NOT NULL DEFAULT NOW(),
            started_at  TIMESTAMP,
            finished_at TIMESTAMP
        );
    ",
        "
        CREATE INDEX IF NOT EXISTS transcode_jobs_claim_idx
            ON transcode_jobs (state, created_at);
    ",
        "
        CREATE TABLE IF NOT EXISTS storage_workers_usages (
            id                 UUID      PRIMARY KEY,
            storage_worker_id  UUID      NOT NULL REFERENCES storage_workers
                                                ON DELETE CASCADE 
                                                ON UPDATE CASCADE,
            dt                 TIMESTAMP DEFAULT NOW()
        );
    ",
    ] {
        sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .inspect_err(|e| {
                tracing::error!("error during initing database with query:\n{statement}\n{e}");
            })
            .unwrap();
    }

    transaction.commit().await.unwrap();
}

#[inline]
pub async fn create_superuser(db: &PgPool, config: &Config) {
    let password_hash = PasswordManager::generate(&config.superuser_pass).unwrap();
    let user = InDBUser::new(config.superuser_email.clone(), password_hash);
    let result = UsersRepository::new(db).create(user).await;

    match result {
        Ok(_) => tracing::debug!("created superuser"),

        // ignoring conflict error -> just skipping it
        Err(XenovraStreamError::AlreadyExists(_)) => {
            tracing::debug!("superuser already exists; skipping")
        }

        // in case of another error kind -> terminating process
        _ => {
            panic!("can't create superuser; terminating process")
        }
    };
}

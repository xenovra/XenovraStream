use reqwest::multipart;
use uuid::Uuid;

use crate::{
    common::types::ChatId,
    errors::{XenovraStreamError, XenovraStreamResult},
    services::storage_workers_scheduler::StorageWorkersScheduler,
};

use super::schemas::{DownloadBodySchema, UploadBodySchema, UploadedChunk};

pub struct TelegramBotApi<'t> {
    base_url: &'t str,
    scheduler: StorageWorkersScheduler<'t>,
}

impl<'t> TelegramBotApi<'t> {
    pub fn new(base_url: &'t str, scheduler: StorageWorkersScheduler<'t>) -> Self {
        Self {
            base_url,
            scheduler,
        }
    }

    /// Normalizes a stored chat id into the form Telegram expects for
    /// channels/supergroups (`-100XXXXXXXXXX`). The id may be stored either in
    /// full form (already carrying the `-100` prefix, e.g. `-1004467284957`) or
    /// without it. We only add the prefix when it is missing, so we never
    /// double-prefix and mangle an already-valid id.
    ///
    /// https://stackoverflow.com/a/65965402/12255756
    fn normalize_chat_id(chat_id: ChatId) -> ChatId {
        let digits = chat_id.abs().to_string();
        let already_prefixed = chat_id < 0 && digits.len() >= 13 && digits.starts_with("100");

        if already_prefixed {
            chat_id
        } else {
            let n = chat_id.abs().checked_ilog10().unwrap_or(0) + 1;
            chat_id - (100 * ChatId::from(10).pow(n))
        }
    }

    pub async fn upload(
        &self,
        file: &[u8],
        chat_id: ChatId,
        storage_id: Uuid,
    ) -> XenovraStreamResult<UploadedChunk> {
        let chat_id = Self::normalize_chat_id(chat_id);

        let token = self.scheduler.get_token(storage_id).await?;
        let url = self.build_url("", "sendDocument", token);

        let file_part = multipart::Part::bytes(file.to_vec()).file_name("xenovrastream_chunk.bin");
        let form = multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("document", file_part);

        let response = reqwest::Client::new()
            .post(url)
            .multipart(form)
            .send()
            .await?;

        match response.error_for_status() {
            // https://stackoverflow.com/a/32679930/12255756
            Ok(r) => {
                let result = r.json::<UploadBodySchema>().await?.result;
                Ok(UploadedChunk {
                    file_id: result.document.file_id,
                    message_id: result.message_id,
                })
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Deletes a previously uploaded chunk message from the Telegram chat.
    ///
    /// Best-effort: a message that is already gone (or can no longer be
    /// deleted) is treated as success so that file deletion stays idempotent
    /// and never leaves a file undeletable in the app.
    pub async fn delete_message(
        &self,
        message_id: i64,
        chat_id: ChatId,
        storage_id: Uuid,
    ) -> XenovraStreamResult<()> {
        let chat_id = Self::normalize_chat_id(chat_id);

        let token = self.scheduler.get_token(storage_id).await?;
        let url = self.build_url("", "deleteMessage", token);

        let response = reqwest::Client::new()
            .post(url)
            .form(&[
                ("chat_id", chat_id.to_string()),
                ("message_id", message_id.to_string()),
            ])
            .send()
            .await?;

        if response.status().is_success() {
            return Ok(());
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        // Tolerate messages that are already gone; surface anything else.
        if body.contains("message to delete not found") || body.contains("message can't be deleted")
        {
            tracing::warn!("[TELEGRAM API] deleteMessage skipped ({status}): {body}");
            Ok(())
        } else {
            tracing::error!("[TELEGRAM API] deleteMessage failed ({status}): {body}");
            Err(XenovraStreamError::TelegramAPIError(format!(
                "deleteMessage {status}"
            )))
        }
    }

    pub async fn download(
        &self,
        telegram_file_id: &str,
        storage_id: Uuid,
    ) -> XenovraStreamResult<Vec<u8>> {
        // getting file path
        let token = self.scheduler.get_token(storage_id).await?;
        let url = self.build_url("", "getFile", token);
        // TODO: add retries with their number taking from env
        let body: DownloadBodySchema = reqwest::Client::new()
            .get(url)
            .query(&[("file_id", telegram_file_id)])
            .send()
            .await?
            .json()
            .await?;

        // downloading the file itself
        let token = self.scheduler.get_token(storage_id).await?;
        let url = self.build_url("file/", &body.result.file_path, token);
        let file = reqwest::get(url)
            .await?
            .bytes()
            .await
            .map(|file| file.to_vec())?;

        Ok(file)
    }

    /// Taking token by a value to force dropping it so it can be used only once
    #[inline]
    fn build_url(&self, pre: &str, relative: &str, token: String) -> String {
        format!("{}/{pre}bot{token}/{relative}", self.base_url)
    }
}

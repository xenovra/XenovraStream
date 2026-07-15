use crate::errors::XenovraStreamError;

#[inline]
pub fn map_not_found(e: sqlx::Error, entity_name: &str) -> XenovraStreamError {
    match e {
        sqlx::Error::RowNotFound => XenovraStreamError::DoesNotExist(format!("such {entity_name}")),
        _ => {
            tracing::error!("{e}");
            XenovraStreamError::Unknown
        }
    }
}

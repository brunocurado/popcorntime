use crate::{error::Error, event::SessionServerReady};
use anyhow::Context;
use popcorntime_error::Code;
use popcorntime_session::AuthorizationService;
use tauri::State;
use tauri_specta::Event;
use tracing::instrument;

#[tauri::command(async)]
#[specta::specta]
#[instrument(skip(handle, service), err(Debug))]
pub async fn initialize_session_authorization(
  handle: tauri::AppHandle,
  service: State<'_, AuthorizationService>,
) -> Result<(), Error> {
  service
    .authorize_in_background(move |event| {
      SessionServerReady {
        authorization_url: event.authorize_url,
      }
      .emit(&handle)
      .context(Code::InvalidEvent)
    })
    .await?;

  Ok(())
}

#[tauri::command(async)]
#[specta::specta]
#[instrument(skip(service), err(Debug))]
pub async fn validate(service: State<'_, AuthorizationService>) -> Result<(), Error> {
  service.validate().await.map_err(Into::into)
}

#[tauri::command(async)]
#[specta::specta]
#[instrument(skip(service), err(Debug))]
pub async fn logout(service: State<'_, AuthorizationService>) -> Result<(), Error> {
  service.logout().await.map_err(Into::into)
}

/// Enter guest mode: offline bypass for when the accounts server is
/// unreachable. Only no-account features (YTS) work as guest.
#[tauri::command(async)]
#[specta::specta]
#[instrument(skip(service), err(Debug))]
pub async fn enable_guest_session(service: State<'_, AuthorizationService>) -> Result<(), Error> {
  service.enable_guest().map_err(Into::into)
}

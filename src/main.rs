use anyhow::{anyhow, Context};
use axum::{
    extract::{Path, State},
    routing::get,
    Router,
};
use shuttle_secrets::SecretStore;

struct AppState {
    airtable_api_key: String,
}

/// Example response from Airtable API:
///
/// ```json
/// {"records": [{
///     "id": "recF5mFCZgsvJtGeh",
///     "createdTime": "2023-10-26T09:49:34.000Z",
///     "fields": {
///         "approval_date": "2023-06-15T22:22:22.776Z",
///         "verification_type": "KYC",
///         "near_wallet": "frol.near",
///         "status": "Approved"
///     }
/// }]}
/// ```
#[derive(serde::Deserialize)]
struct AirtableResponse {
    records: Vec<AirtableRecord>,
}

#[derive(serde::Deserialize)]
struct AirtableRecord {
    id: String,
    #[serde(rename = "createdTime")]
    created_time: String,
    fields: AirtableFields,
}

#[derive(serde::Deserialize)]
struct AirtableFields {
    approval_date: String,
    verification_type: String,
    near_wallet: String,
    status: KycStatus,
}

#[derive(serde::Serialize)]
struct KycResponse {
    account_id: near_account_id::AccountId,
    kyc_status: KycStatus,
}

#[derive(Copy, Clone, serde::Serialize, serde::Deserialize)]
enum KycStatus {
    NotSubmitted,
    Pending,
    Rejected,
    Approved,
}

async fn get_account_kyc_status(
    Path(account_id): Path<near_account_id::AccountId>,
    State(state): State<std::sync::Arc<AppState>>,
) -> anyhow::Result<KycResponse> {
    let body: AirtableResponse = reqwest::Client::new()
        .get("https://api.airtable.com/v0/appjaTXAImNymlY6T/devhub_kyc")
        .query(&[
            ("maxRecords", "3"),
            ("view", "Grid view"),
            (
                "filterByFormula",
                &format!("{{near_wallet}}='{account_id}'"),
            ),
        ])
        .header(
            "Authorization",
            format!("Bearer {}", state.airtable_api_key),
        )
        .send()
        .await
        .context("Failed to fetch data from the database")?
        .json()
        .await
        .context("Failed to deserialized the database data")?;

    Ok(KycResponse {
        account_id,
        kyc_status: body
            .records
            .first()
            .map(|record| record.fields.status)
            .unwrap_or(KycStatus::NotSubmitted),
    })
}

#[shuttle_runtime::main]
async fn main(#[shuttle_secrets::Secrets] secret_store: SecretStore) -> shuttle_axum::ShuttleAxum {
    let airtable_api_key = if let Some(airtable_api_key) = secret_store.get("AIRTABLE_API_KEY") {
        airtable_api_key
    } else {
        return Err(anyhow!("AIRTABLE_API_KEY was not found").into());
    };

    let app_state = std::sync::Arc::new(AppState { airtable_api_key });

    let router = Router::new()
        .route("/kyc/:account_id", get(get_account_kyc_status))
        .with_state(app_state);

    Ok(router.into())
}

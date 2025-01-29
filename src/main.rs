use anyhow::anyhow;
use axum::{
    extract::{Path, State},
    http::Method,
    routing::get,
    Json, Router,
};
use shuttle_runtime::SecretStore;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};

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
///         "status": "pending" / "rejected" / "approved",
///         "approval_standing": "" / "active" / "expired",
///     }
/// }]}
/// ```
#[derive(serde::Deserialize)]
struct AirtableResponse {
    records: Vec<AirtableRecord>,
}

#[derive(serde::Deserialize)]
struct AirtableRecord {
    // id: String,
    // #[serde(rename = "createdTime")]
    // created_time: String,
    fields: AirtableFields,
}

#[derive(serde::Deserialize)]
struct AirtableFields {
    // approval_date: String,
    // verification_type: String,
    // near_wallet: String,
    status: KycStatus,
    #[serde(default = "approval_standing_inactive")]
    approval_standing: KycApprovalStanding,
}

#[derive(serde::Serialize)]
struct KycResponse {
    account_id: near_account_id::AccountId,
    kyc_status: KycStatus,
}

#[derive(Copy, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum KycStatus {
    NotSubmitted,
    #[serde(alias = "pending", alias = "Pending")]
    Pending,
    #[serde(alias = "rejected", alias = "Rejected")]
    Rejected,
    #[serde(alias = "approved", alias = "Approved")]
    Approved,
    Expired,
}

#[derive(Copy, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum KycApprovalStanding {
    Active,
    #[serde(alias = "")]
    Expired,
}

fn approval_standing_inactive() -> KycApprovalStanding {
    KycApprovalStanding::Expired
}

enum KycError {
    DatabaseError,
    DeserializationError,
}

impl axum::response::IntoResponse for KycError {
    fn into_response(self) -> axum::response::Response {
        let body = match self {
            KycError::DatabaseError => "Database error".to_string(),
            KycError::DeserializationError => "Deserialization error".to_string(),
        };

        // its often easiest to implement `IntoResponse` by calling other implementations
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
    }
}

async fn get_account_kyc_status(
    Path(account_id): Path<near_account_id::AccountId>,
    State(state): State<std::sync::Arc<AppState>>,
) -> Result<Json<KycResponse>, KycError> {
    let body: AirtableResponse = reqwest::Client::new()
        .get("https://api.airtable.com/v0/appjaTXAImNymlY6T/devhub_kyc")
        .query(&[
            ("maxRecords", "5"),
            ("view", "Grid view"),
            (
                "filterByFormula",
                &format!("REGEX_MATCH({{near_wallet}}, '(^|,){account_id}(,|$)')"),
            ),
        ])
        .header(
            "Authorization",
            format!("Bearer {}", state.airtable_api_key),
        )
        .send()
        .await
        .map_err(|_| KycError::DatabaseError)?
        .json()
        .await
        .map_err(|_err| {
            dbg!(_err);
            KycError::DeserializationError
        })?;

    Ok(Json(KycResponse {
        account_id,
        kyc_status: if let Some(active_record) = body
            .records
            .iter()
            .filter(|record| matches!(record.fields.approval_standing, KycApprovalStanding::Active))
            .next()
        {
            active_record.fields.status
        } else {
            body.records
                .first()
                .map(|record| {
                    if let KycApprovalStanding::Expired = record.fields.approval_standing {
                        KycStatus::Expired
                    } else {
                        record.fields.status
                    }
                })
                .unwrap_or(KycStatus::NotSubmitted)
        },
    }))
}

#[shuttle_runtime::main]
async fn main(#[shuttle_runtime::Secrets] secret_store: SecretStore) -> shuttle_axum::ShuttleAxum {
    let airtable_api_key = if let Some(airtable_api_key) = secret_store.get("AIRTABLE_API_KEY") {
        airtable_api_key
    } else {
        return Err(anyhow!("AIRTABLE_API_KEY was not found").into());
    };

    let app_state = std::sync::Arc::new(AppState { airtable_api_key });

    let router = Router::new()
        .route("/kyc/:account_id", get(get_account_kyc_status))
        .layer(
            ServiceBuilder::new().layer(
                CorsLayer::new()
                    // allow `GET` and `POST` when accessing the resource
                    .allow_methods([Method::GET, Method::POST])
                    // allow requests from any origin
                    .allow_origin(Any),
            ),
        )
        .with_state(app_state);

    Ok(router.into())
}

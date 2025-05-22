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
/// {
///     "records": [
///         {
///             "id": "recgyIIfWh3f6MPfo",
///             "createdTime": "2025-04-21T01:50:06.000Z",
///             "fields": {
///                 "Wallet Address [Currency]": "[NEAR] petersalomonsen.near",
///                 "Owner Verification Status": "Verified",
///                 "Contact": [
///                     "recw0617NXLJMOUjc"
///                ],
///                 "Wallet Address": "petersalomonsen.near",
///                 "Chain": "NEAR",
///                 "Verification Date": "11/28/2024 1:10am",
///                 "KYC Approval Standing (from Contact)": [
///                     "Approved"
///                 ],
///                 "Final Status": "Verified"
///             }
///         }
///     ]
/// }
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
    #[serde(rename = "Owner Verification Status")]
    approval_standing: KycApprovalStanding,
}

#[derive(serde::Serialize)]
struct KycResponse {
    account_id: near_account_id::AccountId,
    kyc_status: KycStatus,
}

#[derive(Copy, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
enum KycApprovalStanding {
    Verified,
    Rejected,
    Pending,
    Expired,
    #[serde(rename = "Not Submitted")]
    NotSubmitted,
}

impl From<KycApprovalStanding> for KycStatus {
    fn from(approval_standing: KycApprovalStanding) -> Self {
        match approval_standing {
            KycApprovalStanding::Verified => KycStatus::Approved,
            KycApprovalStanding::Rejected => KycStatus::Rejected,
            KycApprovalStanding::Pending => KycStatus::Pending,
            KycApprovalStanding::Expired => KycStatus::Expired,
            KycApprovalStanding::NotSubmitted => KycStatus::NotSubmitted,
        }
    }
}

#[derive(Copy, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum KycStatus {
    NotSubmitted,
    Pending,
    Rejected,
    Approved,
    Expired,
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
        .get("https://api.airtable.com/v0/appc0ZVhbKj8hMLvH/tblIxT2t2gHoZMucn")
        .query(&[
            ("maxRecords", "5"),
            ("view", "Grid view"),
            (
                "filterByFormula",
                &format!("REGEX_MATCH({{Wallet Address}}, '(^|,){account_id}(,|$)')"),
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
            .filter(|record| {
                matches!(
                    record.fields.approval_standing,
                    KycApprovalStanding::Verified
                )
            })
            .next()
        {
            KycStatus::from(active_record.fields.approval_standing)
        } else {
            body.records
                .first()
                .map(|record| KycStatus::from(record.fields.approval_standing))
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

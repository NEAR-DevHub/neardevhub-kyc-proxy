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
    #[serde(rename = "Final Status")]
    approval_standing: KycApprovalStanding,
}

#[derive(serde::Serialize)]
struct KycResponse {
    account_id: near_account_id::AccountId,
    kyc_status: KycApprovalStanding,
}


#[derive(Copy, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
enum KycApprovalStanding {
    Verified,
    Rejected,
    Pending,
    Expired,
    NotSubmitted,
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
    let response = reqwest::Client::new()
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
        .map_err(|_| KycError::DatabaseError)?;

    let raw_json = response.text().await.map_err(|_| KycError::DatabaseError)?;

    let body: AirtableResponse = serde_json::from_str(&raw_json).map_err(|_err| {
        println!("Deserialization error: {:?}", _err);
        println!("Raw JSON response: {}", raw_json);
        KycError::DeserializationError
    })?;

    Ok(Json(KycResponse {
        account_id,
        kyc_status: if let Some(active_record) = body
            .records
            .iter()
            .filter(|record| matches!(record.fields.approval_standing, KycApprovalStanding::Verified))
            .next()
        {
            active_record.fields.approval_standing
        } else {
            body.records
                .first()
                .map(|record| {
                    record.fields.approval_standing
                })
                .unwrap_or(KycApprovalStanding::NotSubmitted)
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

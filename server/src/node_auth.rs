use crate::env::Env;
use crate::http::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{Duration, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct ChallengeRequest {
    pub node_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct ChallengeResponse {
    pub challenge: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
    pub node_id: Uuid,
    pub challenge: String,
    pub signature: String,
}

#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub access_token: String,
    pub expires_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
struct NodeJwtClaims {
    node_id: Uuid,
    exp: usize,
    iat: usize,
}

pub async fn create_auth_challenge(
    State(state): State<AppState>,
    Json(payload): Json<ChallengeRequest>,
) -> Response {
    let node = match state.db.get_node_claim_by_id(&payload.node_id).await {
        Ok(node) => node,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"success": false, "error": "node not found"})),
            )
                .into_response();
        }
    };

    if node.revoked {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"success": false, "error": "node is revoked"})),
        )
            .into_response();
    }

    let challenge = generate_challenge();
    let expires_at = Utc::now() + Duration::seconds(state.env.node_challenge_ttl_secs);

    if let Err(err) = state
        .db
        .upsert_auth_challenge(&node.id, &challenge, expires_at)
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"success": false, "error": err.to_string()})),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(json!(ChallengeResponse { challenge })),
    )
        .into_response()
}

pub async fn verify_auth_challenge(
    State(state): State<AppState>,
    Json(payload): Json<VerifyRequest>,
) -> Response {
    let node = match state.db.get_node_claim_by_id(&payload.node_id).await {
        Ok(node) => node,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"success": false, "error": "node not found"})),
            )
                .into_response();
        }
    };

    if node.revoked {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"success": false, "error": "node is revoked"})),
        )
            .into_response();
    }

    let challenge_record = match state
        .db
        .get_auth_challenge(&payload.node_id, payload.challenge.trim())
        .await
    {
        Ok(Some(record)) => record,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"success": false, "error": "invalid challenge"})),
            )
                .into_response();
        }
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"success": false, "error": err.to_string()})),
            )
                .into_response();
        }
    };

    // Challenges are one-time: consume before cryptographic verification to limit replay attempts.
    if let Err(err) = state
        .db
        .consume_auth_challenge(&payload.node_id, payload.challenge.trim())
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"success": false, "error": err.to_string()})),
        )
            .into_response();
    }

    if challenge_record.expires_at <= Utc::now() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"success": false, "error": "challenge expired"})),
        )
            .into_response();
    }

    if let Err(err) = verify_signature(
        node.public_key.as_str(),
        payload.challenge.trim(),
        payload.signature.trim(),
    ) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"success": false, "error": err.to_string()})),
        )
            .into_response();
    }

    let (access_token, expires_at) = match issue_node_jwt(&state.env, payload.node_id) {
        Ok(result) => result,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"success": false, "error": err.to_string()})),
            )
                .into_response();
        }
    };

    (
        StatusCode::OK,
        Json(json!(VerifyResponse {
            access_token,
            expires_at,
        })),
    )
        .into_response()
}

fn generate_challenge() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn verify_signature(public_key: &str, challenge: &str, signature: &str) -> anyhow::Result<()> {
    let pk = URL_SAFE_NO_PAD
        .decode(public_key)
        .map_err(|_| anyhow::anyhow!("invalid public key encoding"))?;
    let sig = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| anyhow::anyhow!("invalid signature encoding"))?;

    let pk: [u8; 32] = pk
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key must decode to 32 bytes"))?;

    let verifying_key = VerifyingKey::from_bytes(&pk)
        .map_err(|_| anyhow::anyhow!("invalid ed25519 public key"))?;
    let signature = Signature::from_slice(&sig)
        .map_err(|_| anyhow::anyhow!("invalid ed25519 signature"))?;

    verifying_key
        .verify(challenge.as_bytes(), &signature)
        .map_err(|_| anyhow::anyhow!("signature verification failed"))
}

fn issue_node_jwt(env: &Env, node_id: Uuid) -> anyhow::Result<(String, chrono::DateTime<Utc>)> {
    let iat = Utc::now();
    let expires_at = iat + Duration::seconds(env.node_jwt_ttl_secs);

    let claims = NodeJwtClaims {
        node_id,
        exp: expires_at.timestamp() as usize,
        iat: iat.timestamp() as usize,
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(env.node_jwt_secret.as_bytes()),
    )?;

    Ok((token, expires_at))
}

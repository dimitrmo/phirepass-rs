use crate::env::Env;
use argon2::Argon2;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;
use uuid::Uuid;

use chrono::{DateTime, Utc};
// use sqlx::types::Uuid;

pub struct Database {
    pool: PgPool,
    pub hasher: argon2::Argon2<'static>,
}

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
pub struct TokenRecord {
    pub id: Uuid,
    pub token_id: String,
    pub token_hash: String,
    pub user_id: Uuid,
    pub expires_at: Option<DateTime<Utc>>,
    pub scopes: Vec<String>,
}

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
pub struct NodeRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl Database {
    pub async fn create(config: &Env) -> anyhow::Result<Self> {
        let opts = PgConnectOptions::from_str(&config.database_url)?.statement_cache_capacity(0);

        let pool = PgPoolOptions::new()
            .max_connections(config.database_max_connections)
            .connect_with(opts)
            .await?;

        let argon2 = Argon2::default();

        Ok(Self {
            pool,
            hasher: argon2,
        })
    }

    pub async fn create_node_from_token(&self, token: &TokenRecord) -> anyhow::Result<NodeRecord> {
        let name = format!("node-{}", Uuid::new_v4().to_string()[..8].to_string());

        let node_record = sqlx::query_as::<_, NodeRecord>(
            r#"
            INSERT INTO nodes (user_id, token_id, name)
            VALUES ($1, $2, $3)
            RETURNING *
            "#,
        )
        .persistent(false)
        .bind(token.user_id)
        .bind(token.id)
        .bind(name)
        .fetch_one(&self.pool)
        .await?;

        Ok(node_record)
    }

    pub async fn get_token_by_id(&self, token_id: &str) -> anyhow::Result<TokenRecord> {
        let token_record = sqlx::query_as::<_, TokenRecord>(
            r#"
            SELECT *
            FROM pat_tokens
            WHERE token_id = $1
            "#,
        )
        .persistent(false)
        .bind(token_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(token_record)
    }
}

use crate::db::common::NodeRecord;
use crate::db::common::TokenRecord;
use crate::env::Env;
use argon2::Argon2;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;
use uuid::Uuid;

pub struct Database {
    pool: PgPool,
    pub hasher: Argon2<'static>,
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

    pub async fn create_node_from_token_exclusive(
        &self,
        token: &TokenRecord,
    ) -> anyhow::Result<NodeRecord> {
        if let Ok(existing_node) = self.get_node_by_token_id(&token.id).await {
            anyhow::bail!(
                "Token is already in use by node {}. \
                 Please close the existing connection or logout first before using this token again.",
                existing_node.id
            );
        }

        self.create_node_from_token(token).await
    }

    pub async fn get_node_by_id(&self, node_id: &Uuid) -> anyhow::Result<NodeRecord> {
        let node_record = sqlx::query_as::<_, NodeRecord>(
            r#"
            SELECT *
            FROM nodes
            WHERE id = $1
            "#,
        )
        .persistent(false)
        .bind(node_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(node_record)
    }

    pub async fn get_node_by_token_id(&self, token_id: &Uuid) -> anyhow::Result<NodeRecord> {
        let node_record = sqlx::query_as::<_, NodeRecord>(
            r#"
            SELECT *
            FROM nodes
            WHERE token_id = $1
            "#,
        )
        .persistent(false)
        .bind(token_id)
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

    pub async fn delete_node(&self, node_id: &Uuid) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            DELETE FROM nodes
            WHERE id = $1
            "#,
        )
        .persistent(false)
        .bind(node_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

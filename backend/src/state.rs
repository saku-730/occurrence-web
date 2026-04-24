use std::sync::Arc;

use sqlx::PgPool;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub posgre: PgPool,
}

impl AppState {
    pub fn new(config: Config, posgre:PgPool) -> Self {
        Self {
            config: Arc::new(config),
            posgre:posgre,
        }
    }
}
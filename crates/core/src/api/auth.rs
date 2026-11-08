//! `auth.test`.

use serde::{Deserialize, Serialize};

use super::Client;
use crate::Result;

/// Who the session belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthTest {
    pub url: String,
    pub team: String,
    pub user: String,
    pub team_id: String,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enterprise_id: Option<String>,
}

impl Client {
    pub async fn auth_test(&self) -> Result<AuthTest> {
        self.call("auth.test", &[]).await
    }
}
